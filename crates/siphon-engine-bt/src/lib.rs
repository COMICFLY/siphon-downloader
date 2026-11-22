use async_trait::async_trait;
use chrono::Utc;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions};
use percent_encoding::percent_decode_str;
use siphon_core::{
    DownloadContext, DownloadOutcome, EngineUpdate, FileCategory, ProbeMetadata, ProtocolHandler,
    ProtocolKind, Result, SegmentInfo, SegmentState, SiphonError, TaskLogEntry,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

/// Pure-Rust BitTorrent / Magnet download engine powered by `librqbit`.
#[derive(Debug, Default)]
pub struct BitTorrentEngine;

impl BitTorrentEngine {
    pub fn new() -> Self {
        Self
    }
}

pub fn is_bittorrent_url(raw: &str) -> bool {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("magnet:?") {
        return true;
    }
    if lower.starts_with("file://") && lower.ends_with(".torrent") {
        return true;
    }
    if let Ok(parsed) = Url::parse(trimmed) {
        if matches!(parsed.scheme(), "http" | "https") {
            let path_lower = parsed.path().to_ascii_lowercase();
            if path_lower.ends_with(".torrent") {
                return true;
            }
        }
    }
    false
}

pub fn parse_magnet_name(raw: &str) -> String {
    if let Ok(parsed) = Url::parse(raw) {
        for (k, v) in parsed.query_pairs() {
            if k == "dn" && !v.trim().is_empty() {
                return sanitize_bt_name(&v);
            }
        }
        for (k, v) in parsed.query_pairs() {
            if k == "xt" {
                if let Some(hash) = v.rsplit(':').next() {
                    if !hash.trim().is_empty() {
                        return format!("torrent-{}", &hash[..hash.len().min(12)]);
                    }
                }
            }
        }
    }
    "magnet-download".to_string()
}

fn sanitize_bt_name(name: &str) -> String {
    let decoded = percent_decode_str(name).decode_utf8_lossy();
    let cleaned: String = decoded
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "torrent-download".to_string()
    } else {
        trimmed.to_string()
    }
}

fn local_torrent_path_from_url(raw: &str) -> Option<PathBuf> {
    let parsed = Url::parse(raw).ok()?;
    if parsed.scheme() == "file" {
        parsed.to_file_path().ok()
    } else {
        None
    }
}

fn push_log(ctx: &DownloadContext, level: &str, msg: impl Into<String>) {
    let _ = ctx.updates_tx.send(EngineUpdate::Log(TaskLogEntry {
        timestamp: Utc::now(),
        level: level.to_string(),
        message: msg.into(),
    }));
}

#[async_trait]
impl ProtocolHandler for BitTorrentEngine {
    fn kind(&self) -> ProtocolKind {
        ProtocolKind::BitTorrent
    }

    fn supports_url(&self, url: &str) -> bool {
        is_bittorrent_url(url)
    }

    async fn probe(
        &self,
        url: &str,
        headers: &BTreeMap<String, String>,
        _proxy_url: Option<&str>,
        _user_agent: &str,
    ) -> Result<ProbeMetadata> {
        let filename = if url.trim().to_ascii_lowercase().starts_with("magnet:?") {
            parse_magnet_name(url)
        } else if let Some(path) = local_torrent_path_from_url(url) {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("torrent-download");
            sanitize_bt_name(stem)
        } else if let Ok(parsed) = Url::parse(url) {
            let last = parsed
                .path_segments()
                .and_then(|mut s| s.next_back())
                .unwrap_or("torrent-download");
            let trimmed = last
                .strip_suffix(".torrent")
                .or_else(|| last.strip_suffix(".TORRENT"))
                .unwrap_or(last);
            sanitize_bt_name(trimmed)
        } else {
            "torrent-download".to_string()
        };

        let suggested_category = FileCategory::from_filename_and_mime(&filename, None);

        Ok(ProbeMetadata {
            final_url: url.to_string(),
            filename,
            content_length: None,
            content_type: Some("application/x-bittorrent".to_string()),
            supports_range: true,
            etag: None,
            last_modified: None,
            suggested_category,
            resolved_headers: headers.clone(),
            plugin_note: Some("BitTorrent / Magnet P2P".to_string()),
        })
    }

    async fn download(&self, ctx: DownloadContext) -> Result<DownloadOutcome> {
        let output_dir = ctx.task.save_dir.clone();
        tokio::fs::create_dir_all(&output_dir).await?;

        push_log(
            &ctx,
            "info",
            format!("正在初始化 BitTorrent DHT/Peer 会话 -> {}", output_dir.display()),
        );

        let opts = SessionOptions {
            persistence: None,
            fastresume: false,
            ..Default::default()
        };

        let session = Session::new_with_opts(output_dir.clone(), opts)
            .await
            .map_err(|e| SiphonError::Network(format!("BitTorrent 会话初始化失败: {e}")))?;

        let raw_url = ctx.task.original_url.trim().to_string();
        let add_req = if let Some(local_path) = local_torrent_path_from_url(&raw_url) {
            let bytes = tokio::fs::read(&local_path).await.map_err(|e| {
                SiphonError::Network(format!(
                    "无法读取本地种子文件 {}: {e}",
                    local_path.display()
                ))
            })?;
            AddTorrent::from_bytes(bytes)
        } else {
            AddTorrent::from_url(&raw_url)
        };

        let add_opts = AddTorrentOptions {
            overwrite: true,
            ..Default::default()
        };

        let response = tokio::select! {
            _ = ctx.cancel.cancelled() => {
                session.stop().await;
                return Err(SiphonError::Cancelled);
            }
            res = session.add_torrent(add_req, Some(add_opts)) => {
                res.map_err(|e| SiphonError::Network(format!("解析磁力/种子失败: {e}")))?
            }
        };

        let handle = match response {
            AddTorrentResponse::Added(_, h) | AddTorrentResponse::AlreadyManaged(_, h) => h,
            AddTorrentResponse::ListOnly(_) => {
                session.stop().await;
                return Err(SiphonError::Network("种子仅处于列表模式".into()));
            }
        };

        push_log(&ctx, "info", "已加入 BitTorrent Swarm，正在获取元数据与连接节点...");

        let mut metadata_emitted = false;
        let mut last_peers: u32 = 0;

        loop {
            if ctx.cancel.is_cancelled() {
                let _ = session.pause(&handle).await;
                session.stop().await;
                return Err(SiphonError::Cancelled);
            }

            let stats = handle.stats();

            if !metadata_emitted && stats.total_bytes > 0 {
                metadata_emitted = true;
                let resolved_name = handle
                    .name()
                    .map(|n| sanitize_bt_name(&n))
                    .unwrap_or_else(|| ctx.task.filename.clone());
                let category = FileCategory::from_filename_and_mime(&resolved_name, None);
                let _ = ctx.updates_tx.send(EngineUpdate::Probed(ProbeMetadata {
                    final_url: raw_url.clone(),
                    filename: resolved_name.clone(),
                    content_length: Some(stats.total_bytes),
                    content_type: Some("application/x-bittorrent".to_string()),
                    supports_range: true,
                    etag: None,
                    last_modified: None,
                    suggested_category: category,
                    resolved_headers: ctx.task.headers.clone(),
                    plugin_note: Some("BitTorrent / Magnet P2P".to_string()),
                }));
                push_log(
                    &ctx,
                    "info",
                    format!(
                        "元数据就绪: {} ({} bytes)",
                        resolved_name, stats.total_bytes
                    ),
                );
            }

            let (speed_bps, eta_secs, live_peers) = if let Some(live) = stats.live.as_ref() {
                let bps = (live.download_speed.mbps * 1024.0 * 1024.0).max(0.0) as u64;
                let remaining = stats.total_bytes.saturating_sub(stats.progress_bytes);
                let eta = if bps > 0 && stats.total_bytes > 0 {
                    Some(remaining / bps)
                } else {
                    None
                };
                let peers = live.snapshot.peer_stats.live;
                (bps, eta, peers)
            } else {
                (0, None, 0)
            };

            if live_peers != last_peers && live_peers > 0 {
                last_peers = live_peers;
                push_log(&ctx, "info", format!("当前活跃 Peer 节点: {live_peers}"));
            }

            let segments = build_bt_segments(
                stats.total_bytes,
                stats.progress_bytes,
                speed_bps,
                stats.finished,
            );

            let _ = ctx.updates_tx.send(EngineUpdate::Progress {
                downloaded_bytes: stats.progress_bytes,
                total_size: if stats.total_bytes > 0 {
                    Some(stats.total_bytes)
                } else {
                    None
                },
                speed_bps,
                eta_secs,
                segments,
            });

            if let Some(err) = stats.error {
                session.stop().await;
                return Err(SiphonError::Network(format!("BitTorrent 传输错误: {err}")));
            }

            if stats.finished {
                let total_bytes = stats.total_bytes.max(stats.progress_bytes);
                push_log(&ctx, "info", "BitTorrent 全部数据校验并下载完成");
                session.stop().await;
                return Ok(DownloadOutcome {
                    total_bytes,
                    verified_hash: None,
                });
            }

            tokio::select! {
                _ = ctx.cancel.cancelled() => {
                    let _ = session.pause(&handle).await;
                    session.stop().await;
                    return Err(SiphonError::Cancelled);
                }
                _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            }
        }
    }
}

fn build_bt_segments(
    total_bytes: u64,
    downloaded_bytes: u64,
    speed_bps: u64,
    finished: bool,
) -> Vec<SegmentInfo> {
    if total_bytes == 0 {
        return Vec::new();
    }
    const BUCKETS: u64 = 24;
    let chunk = (total_bytes / BUCKETS).max(1);
    let mut out = Vec::with_capacity(BUCKETS as usize);
    for i in 0..BUCKETS {
        let start = i * chunk;
        if start >= total_bytes {
            break;
        }
        let end = if i + 1 == BUCKETS {
            total_bytes - 1
        } else {
            ((i + 1) * chunk - 1).min(total_bytes - 1)
        };
        let cur = if finished {
            end + 1
        } else {
            downloaded_bytes.clamp(start, end + 1)
        };
        let state = if cur > end {
            SegmentState::Done
        } else if cur > start || speed_bps > 0 {
            SegmentState::Active
        } else {
            SegmentState::Pending
        };
        out.push(SegmentInfo {
            index: i as u32,
            start_offset: start,
            current_offset: cur,
            end_offset: end,
            state,
            speed_bps: if state == SegmentState::Active {
                speed_bps
            } else {
                0
            },
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_magnet_and_torrent_urls() {
        let engine = BitTorrentEngine::new();
        assert!(engine.supports_url(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Ubuntu+24.04.iso"
        ));
        assert!(engine.supports_url("https://releases.ubuntu.com/24.04/ubuntu.iso.torrent"));
        assert!(engine.supports_url("file:///C:/Users/Test/Downloads/sample.torrent"));
        assert!(!engine.supports_url("https://example.com/file.zip"));
        assert!(!engine.supports_url("https://example.com/playlist.m3u8"));
    }

    #[tokio::test]
    async fn probes_magnet_display_name() {
        let engine = BitTorrentEngine::new();
        let meta = engine
            .probe(
                "magnet:?xt=urn:btih:abcdef1234567890&dn=Ubuntu%2024.04.2%20Desktop.iso",
                &BTreeMap::new(),
                None,
                "Siphon",
            )
            .await
            .unwrap();
        assert_eq!(meta.filename, "Ubuntu 24.04.2 Desktop.iso");
        assert!(meta.supports_range);
        assert_eq!(meta.suggested_category, FileCategory::DiskImage);
    }
}
