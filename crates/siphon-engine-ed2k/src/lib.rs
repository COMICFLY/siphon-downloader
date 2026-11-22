pub mod link;
pub mod md4;
pub mod server_met;

use async_trait::async_trait;
use chrono::Utc;
use futures_util::StreamExt;
use link::{Ed2kFileLink, is_ed2k_file_link, parse_ed2k_file_link};
use md4::ed2k_file_hash_path;
use server_met::{build_get_sources_packet, build_login_packet};
use siphon_core::{
    DownloadContext, DownloadOutcome, EngineUpdate, FileCategory, ProbeMetadata, ProtocolHandler,
    ProtocolKind, Result, SegmentInfo, SegmentState, SiphonError, TaskLogEntry,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub use link::{parse_ed2k_server_link};
pub use md4::{ed2k_file_hash_hex, md4_digest};
pub use server_met::{parse_server_met_bytes, sync_servers_from_met, test_ed2k_server};

/// Pure-Rust eD2k protocol handler supporting `ed2k://|file|...` parsing, eD2k server
/// source queries, embedded HTTP web-source downloads (`|s=http://...|`), and RFC 1320
/// eD2k MD4 root-hash verification.
#[derive(Debug, Default)]
pub struct Ed2kEngine;

impl Ed2kEngine {
    pub fn new() -> Self {
        Self
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
impl ProtocolHandler for Ed2kEngine {
    fn kind(&self) -> ProtocolKind {
        ProtocolKind::Ed2k
    }

    fn supports_url(&self, url: &str) -> bool {
        is_ed2k_file_link(url)
    }

    async fn probe(
        &self,
        url: &str,
        headers: &BTreeMap<String, String>,
        _proxy_url: Option<&str>,
        _user_agent: &str,
    ) -> Result<ProbeMetadata> {
        let parsed = parse_ed2k_file_link(url)?;
        let suggested_category = FileCategory::from_filename_and_mime(&parsed.filename, None);
        let note = if !parsed.web_sources.is_empty() {
            format!(
                "eD2k MD4 {} · {} 个 Web 来源",
                &parsed.md4_hash[..8],
                parsed.web_sources.len()
            )
        } else {
            format!("eD2k MD4 {}", &parsed.md4_hash[..8])
        };

        Ok(ProbeMetadata {
            final_url: url.trim().to_string(),
            filename: parsed.filename,
            content_length: Some(parsed.file_size),
            content_type: Some("application/x-ed2k".to_string()),
            supports_range: true,
            etag: Some(parsed.md4_hash),
            last_modified: None,
            suggested_category,
            resolved_headers: headers.clone(),
            plugin_note: Some(note),
        })
    }

    async fn download(&self, ctx: DownloadContext) -> Result<DownloadOutcome> {
        if !ctx.ed2k_config.enabled {
            return Err(SiphonError::UnsupportedProtocol(
                "eD2k 协议已在「eD2k 电驴」页面关闭，请先开启后再下载".into(),
            ));
        }

        let parsed = parse_ed2k_file_link(&ctx.task.original_url)?;
        let enabled_servers: Vec<_> = ctx
            .ed2k_config
            .servers
            .iter()
            .filter(|s| s.enabled)
            .cloned()
            .collect();

        push_log(
            &ctx,
            "info",
            format!(
                "解析 eD2k 链接: {} ({} bytes, MD4={}) · 昵称={}, TCP={}, Kad={} (UDP={}), 混淆={}, 启用服务器={}",
                parsed.filename,
                parsed.file_size,
                parsed.md4_hash,
                ctx.ed2k_config.nickname,
                ctx.ed2k_config.tcp_port,
                ctx.ed2k_config.enable_kad,
                ctx.ed2k_config.udp_port,
                ctx.ed2k_config.enable_obfuscation,
                enabled_servers.len(),
            ),
        );

        let _ = ctx.updates_tx.send(EngineUpdate::Probed(ProbeMetadata {
            final_url: ctx.task.original_url.clone(),
            filename: ctx.task.filename.clone(),
            content_length: Some(parsed.file_size),
            content_type: Some("application/x-ed2k".to_string()),
            supports_range: true,
            etag: Some(parsed.md4_hash.clone()),
            last_modified: None,
            suggested_category: FileCategory::from_filename_and_mime(&ctx.task.filename, None),
            resolved_headers: ctx.task.headers.clone(),
            plugin_note: Some(format!("eD2k MD4 {}", &parsed.md4_hash[..8])),
        }));

        // Path 1: Embedded HTTP/HTTPS web-sources (`|s=https://...|`) — fast direct transfer + MD4 verification
        if !parsed.web_sources.is_empty() {
            return download_via_web_sources(&ctx, &parsed).await;
        }

        // Path 2: Query configured eD2k servers with OP_LOGINREQUEST + OP_GETSOURCES
        let mut md4_bytes = [0u8; 16];
        if let Ok(decoded) = hex::decode(&parsed.md4_hash)
            && decoded.len() == 16
        {
            md4_bytes.copy_from_slice(&decoded);
        }

        let mut connected_servers = 0usize;
        let mut discovered_peers: Vec<(String, u16)> = parsed.peer_sources.clone();

        for srv in enabled_servers.iter().take(5) {
            if ctx.cancel.is_cancelled() {
                return Err(SiphonError::Cancelled);
            }
            push_log(
                &ctx,
                "info",
                format!("正在连接 eD2k 服务器 {} ({}:{})...", srv.name, srv.host, srv.port),
            );
            match query_ed2k_server_sources(
                &srv.host,
                srv.port,
                &ctx.ed2k_config.nickname,
                ctx.ed2k_config.tcp_port,
                &md4_bytes,
                parsed.file_size,
            )
            .await
            {
                Ok(peers) => {
                    connected_servers += 1;
                    push_log(
                        &ctx,
                        "info",
                        format!(
                            "服务器 {} 握手成功，返回 {} 个源节点",
                            srv.name,
                            peers.len()
                        ),
                    );
                    discovered_peers.extend(peers);
                }
                Err(e) => {
                    push_log(&ctx, "warn", format!("服务器 {} 连接失败: {e}", srv.name));
                }
            }
        }

        Err(SiphonError::Network(format!(
            "已完成 {} 台 eD2k 服务器来源查询（发现 {} 个 P2P 源节点），当前 eD2k 链接不含可用 Web 来源且无活跃直连 Peer；可在「eD2k 电驴」页面同步最新 server.met 或使用带 |s=...| 来源的 eD2k 链接",
            connected_servers,
            discovered_peers.len()
        )))
    }
}

async fn query_ed2k_server_sources(
    host: &str,
    port: u16,
    nickname: &str,
    tcp_port: u16,
    md4_bytes: &[u8; 16],
    file_size: u64,
) -> std::result::Result<Vec<(String, u16)>, String> {
    let addr = format!("{host}:{port}");
    let mut stream = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(&addr))
        .await
        .map_err(|_| "连接超时".to_string())?
        .map_err(|e| e.to_string())?;

    let login = build_login_packet(nickname, tcp_port);
    let query = build_get_sources_packet(md4_bytes, file_size);
    stream
        .write_all(&login)
        .await
        .map_err(|e| format!("发送登录包失败: {e}"))?;
    stream
        .write_all(&query)
        .await
        .map_err(|e| format!("发送来源查询包失败: {e}"))?;

    let mut peers = Vec::new();
    let mut buf = vec![0u8; 4096];
    if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf)).await
        && n >= 6
        && buf[0] == 0xE3
        && buf[5] == 0x42
        && n >= 23
    {
        // OP_FOUNDSOURCES: 0xE3 + u32 len + 0x42 + 16-byte hash + u8 count + count * (4B IP + 2B port)
        let count = buf[22] as usize;
        let mut pos = 23usize;
        for _ in 0..count {
            if pos + 6 > n {
                break;
            }
            let ip = format!(
                "{}.{}.{}.{}",
                buf[pos],
                buf[pos + 1],
                buf[pos + 2],
                buf[pos + 3]
            );
            let p = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]);
            if p > 0 {
                peers.push((ip, p));
            }
            pos += 6;
        }
    }
    Ok(peers)
}

async fn download_via_web_sources(
    ctx: &DownloadContext,
    parsed: &Ed2kFileLink,
) -> Result<DownloadOutcome> {
    tokio::fs::create_dir_all(&ctx.task.save_dir).await?;
    let part_path = ctx.task.part_path();
    let target_path = ctx.task.target_path();

    let mut builder = reqwest::Client::builder()
        .user_agent(ctx.user_agent.clone())
        .tcp_nodelay(true)
        .timeout(Duration::from_secs(60));
    if let Some(ref proxy) = ctx.proxy_url
        && !proxy.trim().is_empty()
        && let Ok(p) = reqwest::Proxy::all(proxy.trim())
    {
        builder = builder.proxy(p);
    }
    let client = builder
        .build()
        .map_err(|e| SiphonError::Network(format!("初始化 HTTP 客户端失败: {e}")))?;

    let mut last_err = "无可用 eD2k Web 来源".to_string();

    for source_url in &parsed.web_sources {
        if ctx.cancel.is_cancelled() {
            return Err(SiphonError::Cancelled);
        }
        push_log(ctx, "info", format!("正在从 eD2k Web 来源下载: {source_url}"));

        let existing_len = tokio::fs::metadata(&part_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        let mut req = client.get(source_url);
        if existing_len > 0 && existing_len < parsed.file_size {
            req = req.header("Range", format!("bytes={existing_len}-"));
        }

        let resp = match req.send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                last_err = format!("Web 来源返回 HTTP {}", r.status());
                continue;
            }
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };

        let resuming = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT && existing_len > 0;
        let mut downloaded = if resuming { existing_len } else { 0 };
        let mut file = if resuming {
            tokio::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(&part_path)
                .await?
        } else {
            tokio::fs::File::create(&part_path).await?
        };

        let mut stream = resp.bytes_stream();
        let mut last_tick = Instant::now();
        let mut bytes_since_tick = 0u64;

        while let Some(chunk_res) = tokio::select! {
            _ = ctx.cancel.cancelled() => {
                let _ = file.flush().await;
                return Err(SiphonError::Cancelled);
            }
            item = stream.next() => item,
        } {
            let chunk = chunk_res.map_err(|e| SiphonError::Network(e.to_string()))?;
            ctx.limiter.consume(chunk.len() as u64, &ctx.cancel).await;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            bytes_since_tick += chunk.len() as u64;

            let elapsed = last_tick.elapsed();
            if elapsed >= Duration::from_millis(200) {
                let speed_bps = ((bytes_since_tick as f64) / elapsed.as_secs_f64()) as u64;
                let total = parsed.file_size.max(downloaded);
                let eta_secs = if speed_bps > 0 && total > downloaded {
                    Some((total - downloaded) / speed_bps)
                } else {
                    None
                };
                let _ = ctx.updates_tx.send(EngineUpdate::Progress {
                    downloaded_bytes: downloaded,
                    total_size: Some(total),
                    speed_bps,
                    eta_secs,
                    segments: build_ed2k_segments(total, downloaded, speed_bps, false),
                });
                last_tick = Instant::now();
                bytes_since_tick = 0;
            }
        }

        file.flush().await?;
        drop(file);

        // Verify eD2k MD4 hash when enabled
        let mut verified_hash = None;
        if ctx.ed2k_config.verify_md4_hash {
            let _ = ctx.updates_tx.send(EngineUpdate::Verifying);
            push_log(ctx, "info", "正在执行 eD2k MD4 完整性校验...");
            let part_clone = part_path.clone();
            let actual = tokio::task::spawn_blocking(move || ed2k_file_hash_path(&part_clone))
                .await
                .map_err(|e| SiphonError::Network(format!("MD4 校验任务异常: {e}")))??;
            if !actual.eq_ignore_ascii_case(&parsed.md4_hash) {
                let _ = tokio::fs::remove_file(&part_path).await;
                return Err(SiphonError::ChecksumMismatch {
                    expected: parsed.md4_hash.clone(),
                    actual,
                });
            }
            push_log(ctx, "info", format!("eD2k MD4 校验通过: {actual}"));
            verified_hash = Some(actual);
        }

        if target_path.exists() {
            let _ = tokio::fs::remove_file(&target_path).await;
        }
        tokio::fs::rename(&part_path, &target_path).await?;

        let final_total = parsed.file_size.max(downloaded);
        let _ = ctx.updates_tx.send(EngineUpdate::Progress {
            downloaded_bytes: final_total,
            total_size: Some(final_total),
            speed_bps: 0,
            eta_secs: Some(0),
            segments: build_ed2k_segments(final_total, final_total, 0, true),
        });

        return Ok(DownloadOutcome {
            total_bytes: final_total,
            verified_hash,
        });
    }

    Err(SiphonError::Network(last_err))
}

fn build_ed2k_segments(
    total_bytes: u64,
    downloaded_bytes: u64,
    speed_bps: u64,
    finished: bool,
) -> Vec<SegmentInfo> {
    if total_bytes == 0 {
        return Vec::new();
    }
    const BUCKETS: u64 = 16;
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
    use siphon_core::{DownloadManager, NewTaskRequest, TaskSource, TaskStatus};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ed2k_web_source_downloads_and_verifies_md4_hash() {
        let payload = vec![0x5Au8; 64 * 1024 + 321];
        let expected_md4 = ed2k_file_hash_hex(&payload);
        let payload_for_route = payload.clone();

        let app = Router::new().route(
            "/mirror/demo.bin",
            get(move || {
                let body = payload_for_route.clone();
                async move { body }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let tmp = tempfile::tempdir().unwrap();
        let mgr = DownloadManager::new_in_memory(
            tmp.path().to_path_buf(),
            vec![Arc::new(Ed2kEngine::new())],
        )
        .unwrap();

        let ed2k_url = format!(
            "ed2k://|file|demo_archive.bin|{}|{}|s=http://{addr}/mirror/demo.bin|/",
            payload.len(),
            expected_md4
        );

        let task = mgr
            .add_task(NewTaskRequest::quick(ed2k_url, TaskSource::Manual))
            .await
            .unwrap();
        assert_eq!(task.protocol, ProtocolKind::Ed2k);

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let cur = mgr.get_task(task.id).await.unwrap();
            if cur.status == TaskStatus::Completed {
                let on_disk = std::fs::read(cur.target_path()).unwrap();
                assert_eq!(on_disk, payload);
                break;
            }
            if cur.status == TaskStatus::Error {
                panic!("eD2k download failed: {:?}", cur.error_message);
            }
            assert!(tokio::time::Instant::now() < deadline, "eD2k timed out");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ed2k_md4_mismatch_rejects_corrupted_file() {
        let payload = b"corrupted payload bytes".to_vec();
        let wrong_md4 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let payload_for_route = payload.clone();

        let app = Router::new().route(
            "/bad.bin",
            get(move || {
                let body = payload_for_route.clone();
                async move { body }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let tmp = tempfile::tempdir().unwrap();
        let mgr = DownloadManager::new_in_memory(
            tmp.path().to_path_buf(),
            vec![Arc::new(Ed2kEngine::new())],
        )
        .unwrap();

        let ed2k_url = format!(
            "ed2k://|file|bad.bin|{}|{}|s=http://{addr}/bad.bin|/",
            payload.len(),
            wrong_md4
        );
        let task = mgr
            .add_task(NewTaskRequest::quick(ed2k_url, TaskSource::Manual))
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let cur = mgr.get_task(task.id).await.unwrap();
            if cur.status == TaskStatus::Error {
                assert!(
                    cur.error_message
                        .as_deref()
                        .unwrap_or("")
                        .contains("校验和不匹配"),
                    "unexpected error: {:?}",
                    cur.error_message
                );
                break;
            }
            assert!(cur.status != TaskStatus::Completed, "must reject bad MD4");
            assert!(tokio::time::Instant::now() < deadline, "timed out");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
}
