use async_trait::async_trait;
use chrono::Utc;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ConnectionOptions, DhtSessionConfig,
    ListenerOptions, PeerConnectionOptions, Session, SessionOptions,
};
use percent_encoding::percent_decode_str;
use siphon_core::{
    BtConfig, DownloadContext, DownloadOutcome, EngineUpdate, FileCategory, ProbeMetadata,
    ProtocolHandler, ProtocolKind, Result, SegmentInfo, SegmentState, SiphonError, TaskLogEntry,
    parse_tracker_list,
};
use std::collections::{BTreeMap, HashSet};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

/// Pure-Rust BitTorrent / Magnet download engine powered by `librqbit`,
/// with full custom tracker injection, subscription sync, DHT/LSD/UPnP and port controls.
#[derive(Debug, Default)]
pub struct BitTorrentEngine;

impl BitTorrentEngine {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
pub struct TrackerSyncReport {
    pub trackers: Vec<String>,
    pub sources_succeeded: usize,
    pub sources_failed: usize,
}

/// Download and merge public BitTorrent tracker lists from one or more HTTP/HTTPS subscription URLs.
pub async fn sync_trackers_from_urls(
    urls: &[String],
    proxy_url: Option<&str>,
) -> std::result::Result<TrackerSyncReport, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(format!("Siphon/{}", env!("CARGO_PKG_VERSION")));
    if let Some(proxy) = proxy_url
        && !proxy.trim().is_empty()
        && let Ok(p) = reqwest::Proxy::all(proxy.trim())
    {
        builder = builder.proxy(p);
    }
    let client = builder
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))?;

    let mut combined_raw = String::new();
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut last_err = String::new();

    for raw_url in urls {
        let trimmed = raw_url.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match client.get(trimmed).send().await {
            Ok(resp) if resp.status().is_success() => match resp.text().await {
                Ok(body) => {
                    combined_raw.push_str(&body);
                    combined_raw.push('\n');
                    succeeded += 1;
                }
                Err(e) => {
                    failed += 1;
                    last_err = e.to_string();
                }
            },
            Ok(resp) => {
                failed += 1;
                last_err = format!("HTTP {}", resp.status());
            }
            Err(e) => {
                failed += 1;
                last_err = e.to_string();
            }
        }
    }

    if succeeded == 0 {
        return Err(if last_err.is_empty() {
            "未提供有效的 Tracker 订阅源地址".to_string()
        } else {
            format!("同步 Tracker 订阅源失败: {last_err}")
        });
    }

    let trackers = parse_tracker_list(&combined_raw);
    Ok(TrackerSyncReport {
        trackers,
        sources_succeeded: succeeded,
        sources_failed: failed,
    })
}

/// Build `librqbit::SessionOptions` directly from Siphon's `BtConfig` + optional proxy URL.
pub fn build_session_options(bt: &BtConfig, proxy_url: Option<&str>) -> SessionOptions {
    let dht = if bt.enable_dht {
        Some(DhtSessionConfig {
            persistence: None,
            ..Default::default()
        })
    } else {
        None
    };

    let listen = if bt.listen_port > 0 || bt.enable_upnp {
        Some(ListenerOptions {
            listen_addr: (Ipv4Addr::UNSPECIFIED, bt.listen_port).into(),
            enable_upnp_port_forwarding: bt.enable_upnp,
            ..Default::default()
        })
    } else {
        None
    };

    let socks_proxy = proxy_url
        .map(str::trim)
        .filter(|p| p.to_ascii_lowercase().starts_with("socks5://"))
        .map(str::to_string);

    let connect = Some(ConnectionOptions {
        enable_tcp: true,
        proxy_url: socks_proxy,
        peer_opts: Some(PeerConnectionOptions {
            connect_timeout: Some(Duration::from_secs(bt.peer_connect_timeout_secs.clamp(3, 60))),
            ..Default::default()
        }),
    });

    let mut trackers = HashSet::new();
    if bt.auto_append_trackers && !bt.disable_trackers {
        for raw in bt.effective_trackers() {
            if let Ok(u) = Url::parse(&raw) {
                trackers.insert(u);
            }
        }
    }

    SessionOptions {
        dht,
        disable_trackers: bt.disable_trackers,
        disable_local_service_discovery: !bt.enable_lsd,
        fastresume: false,
        persistence: None,
        listen,
        connect,
        trackers,
        peer_limit: Some(bt.max_peers_per_torrent.clamp(10, 500)),
        client_name_and_version: Some(format!("Siphon/{}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    }
}

/// Build `librqbit::AddTorrentOptions` with custom/synced trackers injected when enabled.
pub fn build_add_torrent_options(bt: &BtConfig) -> AddTorrentOptions {
    let trackers = if bt.auto_append_trackers && !bt.disable_trackers {
        let list = bt.effective_trackers();
        if list.is_empty() { None } else { Some(list) }
    } else {
        None
    };

    AddTorrentOptions {
        overwrite: true,
        disable_trackers: bt.disable_trackers,
        peer_limit: Some(bt.max_peers_per_torrent.clamp(10, 500)),
        trackers,
        ..Default::default()
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

        let opts = build_session_options(&ctx.bt_config, ctx.proxy_url.as_deref());
        let tracker_count = opts.trackers.len();
        push_log(
            &ctx,
            "info",
            format!(
                "初始化 BitTorrent 会话（DHT={}, LSD={}, UPnP={}, 注入自定义 Tracker={} 个）-> {}",
                ctx.bt_config.enable_dht,
                ctx.bt_config.enable_lsd,
                ctx.bt_config.enable_upnp,
                tracker_count,
                output_dir.display()
            ),
        );

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

        let add_opts = build_add_torrent_options(&ctx.bt_config);

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
    use axum::Router;
    use axum::routing::get;
    use tokio::net::TcpListener;

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
        assert!(!engine.supports_url("ed2k://|file|test.iso|1024|31D6CFE0D16AE931B73C59D7E0C089C0|/"));
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

    #[test]
    fn build_session_and_add_options_applies_custom_trackers_and_network_flags() {
        let bt = BtConfig {
            enable_dht: false,
            enable_lsd: false,
            enable_upnp: true,
            disable_trackers: false,
            listen_port: 6889,
            peer_connect_timeout_secs: 15,
            max_peers_per_torrent: 200,
            auto_append_trackers: true,
            custom_trackers: vec![
                "udp://tracker.example.org:1337/announce".into(),
                "https://tracker.example.net/announce".into(),
            ],
            synced_trackers: vec![
                "udp://tracker.example.org:1337/announce".into(), // duplicate
                "udp://synced.example.com:80/announce".into(),
            ],
            ..Default::default()
        };

        let sess = build_session_options(&bt, Some("socks5://127.0.0.1:1080"));
        assert!(sess.dht.is_none());
        assert!(sess.disable_local_service_discovery);
        assert_eq!(sess.peer_limit, Some(200));
        assert_eq!(sess.trackers.len(), 3);
        let listen = sess.listen.expect("listen options present");
        assert_eq!(listen.listen_addr.port(), 6889);
        assert!(listen.enable_upnp_port_forwarding);
        assert_eq!(
            sess.connect.as_ref().and_then(|c| c.proxy_url.as_deref()),
            Some("socks5://127.0.0.1:1080")
        );

        let add = build_add_torrent_options(&bt);
        let list = add.trackers.expect("trackers injected");
        assert_eq!(list.len(), 3);
        assert_eq!(list[0], "udp://tracker.example.org:1337/announce");
        assert_eq!(list[2], "udp://synced.example.com:80/announce");
    }

    #[tokio::test]
    async fn sync_trackers_from_local_http_subscription_deduplicates_and_handles_partial_failure() {
        let list_a = "# Best Trackers\nudp://t1.example:1337/announce\nhttps://t2.example/announce\n";
        let list_b = "https://t2.example/announce\nudp://t3.example:6969/announce\ninvalid_line\n";
        let app = Router::new()
            .route("/best.txt", get(move || async move { list_a }))
            .route("/extra.txt", get(move || async move { list_b }));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let report = sync_trackers_from_urls(
            &[
                format!("http://{addr}/best.txt"),
                format!("http://{addr}/extra.txt"),
                format!("http://{addr}/missing-404.txt"),
            ],
            None,
        )
        .await
        .unwrap();

        assert_eq!(report.sources_succeeded, 2);
        assert_eq!(report.sources_failed, 1);
        assert_eq!(
            report.trackers,
            vec![
                "udp://t1.example:1337/announce",
                "https://t2.example/announce",
                "udp://t3.example:6969/announce",
            ]
        );
    }
}
