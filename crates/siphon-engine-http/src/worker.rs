use crate::file_io::{finalize_part_file, open_worker_file, prepare_part_file, write_all_at};
use crate::probe::{build_client, build_header_map, probe_http};
use crate::splitter::{LiveSegment, SegmentCoordinator};
use crate::verifier::verify_file_checksum;
use chrono::Utc;
use futures_util::StreamExt;
use reqwest::header::{HeaderValue, IF_RANGE, RANGE};
use reqwest::{Client, StatusCode};
use siphon_core::{
    DownloadContext, DownloadOutcome, EngineUpdate, Result, SegmentState, SiphonError,
    TaskLogEntry, deduplicate_filename, format_bytes, sanitize_filename,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const MAX_RETRIES_PER_SEGMENT: u32 = 6;

fn send_log(tx: &mpsc::UnboundedSender<EngineUpdate>, level: &str, msg: impl Into<String>) {
    let _ = tx.send(EngineUpdate::Log(TaskLogEntry {
        timestamp: Utc::now(),
        level: level.to_string(),
        message: msg.into(),
    }));
}

pub async fn run_http_download(ctx: DownloadContext) -> Result<DownloadOutcome> {
    let task = &ctx.task;
    let cancel = ctx.cancel.clone();

    send_log(
        &ctx.updates_tx,
        "INFO",
        format!("开始探测目标地址: {}", task.resolved_url),
    );

    let mut meta = tokio::select! {
        _ = cancel.cancelled() => return Err(SiphonError::Cancelled),
        res = probe_http(
            &task.resolved_url,
            &task.headers,
            ctx.proxy_url.as_deref(),
            &ctx.user_agent,
        ) => res?,
    };

    // Determine whether we are resuming an existing partial download
    let mut existing_segments = task.segments.clone();
    let initial_part_path = task.part_path();
    let resuming = !existing_segments.is_empty() && initial_part_path.exists();

    if resuming {
        // Verify remote file consistency on resume
        if let (Some(old_etag), Some(new_etag)) = (&task.etag, &meta.etag)
            && old_etag != new_etag
            && task.original_url == task.resolved_url
        {
            return Err(SiphonError::RemoteFileChanged);
        }
        if let (Some(old_len), Some(new_len)) = (task.total_size, meta.content_length)
            && old_len != new_len
        {
            return Err(SiphonError::RemoteFileChanged);
        }
        // Keep existing filename untouched when resuming
        meta.filename = task.filename.clone();
    } else {
        existing_segments.clear();
        // If quick-added without explicit filename probing, adopt probed filename if cleaner
        let cleaned = sanitize_filename(&meta.filename);
        if !cleaned.is_empty() && cleaned != task.filename {
            meta.filename = deduplicate_filename(&task.save_dir, &cleaned);
        } else {
            meta.filename = task.filename.clone();
        }
    }

    let final_filename = meta.filename.clone();
    let target_path = task.save_dir.join(&final_filename);
    let part_path = task.save_dir.join(format!("{final_filename}.siphon.part"));

    let _ = ctx.updates_tx.send(EngineUpdate::Probed(meta.clone()));

    let size_label = meta
        .content_length
        .map(format_bytes)
        .unwrap_or_else(|| "未知大小".to_string());
    send_log(
        &ctx.updates_tx,
        "INFO",
        format!(
            "探测完成: {} | 大小: {} | 断点续传: {}",
            final_filename,
            size_label,
            if meta.supports_range { "支持" } else { "不支持（单线程）" }
        ),
    );

    // Handle 0-byte empty file cleanly
    if meta.content_length == Some(0) {
        prepare_part_file(&part_path, Some(0))?;
        finalize_part_file(&part_path, &target_path)?;
        return Ok(DownloadOutcome {
            total_bytes: 0,
            verified_hash: None,
        });
    }

    prepare_part_file(&part_path, meta.content_length)?;

    let initial_conns = ctx
        .live_max_connections
        .load(Ordering::Relaxed)
        .clamp(1, 32);
    let coordinator = SegmentCoordinator::initialize(
        &existing_segments,
        meta.content_length,
        meta.supports_range,
        initial_conns,
        ctx.min_split_size_bytes,
    );

    let worker_count = if meta.supports_range {
        initial_conns as usize
    } else {
        1
    };

    let client = build_client(ctx.proxy_url.as_deref(), &ctx.user_agent)?;
    let custom_headers = Arc::new(build_header_map(&task.headers));
    let if_range_val = meta.etag.clone().or_else(|| meta.last_modified.clone());
    let final_url = Arc::new(meta.final_url.clone());

    // Spawn periodic 250ms telemetry & progress emitter
    let reporter_coord = Arc::clone(&coordinator);
    let reporter_tx = ctx.updates_tx.clone();
    let reporter_cancel = CancellationToken::new();
    let reporter_stop = reporter_cancel.clone();
    let reporter_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        let mut last_tick = Instant::now();
        let mut smoothed_speed: f64 = 0.0;
        loop {
            tokio::select! {
                _ = reporter_stop.cancelled() => break,
                _ = interval.tick() => {
                    let now = Instant::now();
                    let dt = now.duration_since(last_tick).as_secs_f64().max(0.05);
                    last_tick = now;
                    let (downloaded, instant_speed, segments) = reporter_coord.snapshot(dt);
                    // EMA smoothing for stable UI speed readout & chart
                    smoothed_speed = if smoothed_speed <= 1.0 {
                        instant_speed as f64
                    } else {
                        smoothed_speed * 0.65 + instant_speed as f64 * 0.35
                    };
                    let speed_bps = smoothed_speed.round() as u64;
                    let total_size = reporter_coord.total_size();
                    let eta_secs = match (total_size, speed_bps) {
                        (Some(tot), spd) if spd > 0 && tot > downloaded => {
                            Some((tot - downloaded) / spd)
                        }
                        (Some(tot), _) if tot <= downloaded => Some(0),
                        _ => None,
                    };
                    let _ = reporter_tx.send(EngineUpdate::Progress {
                        downloaded_bytes: downloaded,
                        total_size,
                        speed_bps,
                        eta_secs,
                        segments,
                    });
                }
            }
        }
    });

    // Shared internal cancellation token so a fatal error in one worker stops peers cleanly
    let workers_cancel = cancel.child_token();
    let mut handles = Vec::with_capacity(worker_count);

    for worker_id in 0..worker_count {
        let coord = Arc::clone(&coordinator);
        let client = client.clone();
        let url = Arc::clone(&final_url);
        let headers = Arc::clone(&custom_headers);
        let part = part_path.clone();
        let limiter = ctx.limiter.clone();
        let w_cancel = workers_cancel.clone();
        let updates_tx = ctx.updates_tx.clone();
        let if_range = if_range_val.clone();

        handles.push(tokio::spawn(async move {
            worker_loop(
                worker_id,
                coord,
                client,
                url,
                headers,
                if_range,
                part,
                limiter,
                w_cancel,
                updates_tx,
            )
            .await
        }));
    }

    let mut first_error: Option<SiphonError> = None;
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(SiphonError::Cancelled)) => {
                if first_error.is_none() {
                    first_error = Some(SiphonError::Cancelled);
                }
            }
            Ok(Err(e)) => {
                workers_cancel.cancel();
                if first_error.is_none() || matches!(first_error, Some(SiphonError::Cancelled)) {
                    first_error = Some(e);
                }
            }
            Err(join_err) => {
                workers_cancel.cancel();
                if first_error.is_none() {
                    first_error = Some(SiphonError::Network(format!("工作协程异常: {join_err}")));
                }
            }
        }
    }

    reporter_cancel.cancel();
    let _ = reporter_handle.await;

    // Emit final accurate snapshot
    let (downloaded, _, segments) = coordinator.snapshot(0.25);
    let _ = ctx.updates_tx.send(EngineUpdate::Progress {
        downloaded_bytes: downloaded,
        total_size: meta.content_length.or(Some(downloaded)),
        speed_bps: 0,
        eta_secs: if first_error.is_none() { Some(0) } else { None },
        segments,
    });

    if let Some(err) = first_error {
        return Err(err);
    }

    if !coordinator.all_done() {
        if cancel.is_cancelled() {
            return Err(SiphonError::Cancelled);
        }
        return Err(SiphonError::Network("部分分片未能完成下载".to_string()));
    }

    // Optional post-download checksum verification before finalizing
    let verified_hash = if let Some(spec) = task.checksum.clone() {
        let _ = ctx.updates_tx.send(EngineUpdate::Verifying);
        send_log(&ctx.updates_tx, "INFO", "正在执行文件完整性哈希校验...");
        let hash = verify_file_checksum(part_path.clone(), spec).await?;
        send_log(&ctx.updates_tx, "INFO", format!("哈希校验通过: {hash}"));
        Some(hash)
    } else {
        None
    };

    finalize_part_file(&part_path, &target_path)?;

    let final_bytes = meta.content_length.unwrap_or(downloaded);
    Ok(DownloadOutcome {
        total_bytes: final_bytes,
        verified_hash,
    })
}

#[allow(clippy::too_many_arguments)]
async fn worker_loop(
    worker_id: usize,
    coord: Arc<SegmentCoordinator>,
    client: Client,
    url: Arc<String>,
    custom_headers: Arc<reqwest::header::HeaderMap>,
    if_range_val: Option<String>,
    part_path: PathBuf,
    limiter: siphon_core::HierarchicalLimiter,
    cancel: CancellationToken,
    updates_tx: mpsc::UnboundedSender<EngineUpdate>,
) -> Result<()> {
    let file = open_worker_file(&part_path)?;

    while !cancel.is_cancelled() {
        let Some((seg, was_stolen)) = coord.claim_or_steal() else {
            break;
        };

        if was_stolen {
            let (cur, end) = seg.current_and_end();
            send_log(
                &updates_tx,
                "INFO",
                format!(
                    "动态借片: Worker #{worker_id} 接管分片 #{} ({} - {})",
                    seg.index,
                    format_bytes(cur),
                    format_bytes(end)
                ),
            );
        }

        download_segment_with_retries(
            worker_id,
            &seg,
            coord.supports_range(),
            &client,
            &url,
            &custom_headers,
            if_range_val.as_deref(),
            &file,
            &part_path,
            &limiter,
            &cancel,
            &updates_tx,
        )
        .await?;
    }

    if cancel.is_cancelled() {
        Err(SiphonError::Cancelled)
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn download_segment_with_retries(
    worker_id: usize,
    seg: &Arc<LiveSegment>,
    supports_range: bool,
    client: &Client,
    url: &str,
    custom_headers: &reqwest::header::HeaderMap,
    if_range_val: Option<&str>,
    file: &std::fs::File,
    _part_path: &Path,
    limiter: &siphon_core::HierarchicalLimiter,
    cancel: &CancellationToken,
    updates_tx: &mpsc::UnboundedSender<EngineUpdate>,
) -> Result<()> {
    let mut attempt = 0u32;

    loop {
        if cancel.is_cancelled() {
            seg.set_state(SegmentState::Pending);
            return Err(SiphonError::Cancelled);
        }

        let (cur, end) = seg.current_and_end();
        if end != u64::MAX && cur > end {
            seg.set_state(SegmentState::Done);
            return Ok(());
        }

        seg.set_state(SegmentState::Active);

        let attempt_res = download_segment_once(
            seg,
            supports_range,
            client,
            url,
            custom_headers,
            if_range_val,
            file,
            limiter,
            cancel,
        )
        .await;

        match attempt_res {
            Ok(()) => {
                seg.set_state(SegmentState::Done);
                return Ok(());
            }
            Err(SiphonError::Cancelled) => {
                seg.set_state(SegmentState::Pending);
                return Err(SiphonError::Cancelled);
            }
            Err(err @ SiphonError::LinkExpired(_)) | Err(err @ SiphonError::RemoteFileChanged) => {
                seg.set_state(SegmentState::Error);
                return Err(err);
            }
            Err(err) => {
                attempt += 1;
                if attempt > MAX_RETRIES_PER_SEGMENT {
                    seg.set_state(SegmentState::Error);
                    return Err(err);
                }
                seg.set_state(SegmentState::Retrying);
                let backoff_ms = (500u64 << (attempt - 1).min(4)).min(8_000);
                send_log(
                    updates_tx,
                    "WARN",
                    format!(
                        "分片 #{} (Worker #{worker_id}) 异常: {err}，{}ms 后第 {attempt} 次重试",
                        seg.index, backoff_ms
                    ),
                );
                tokio::select! {
                    _ = cancel.cancelled() => {
                        seg.set_state(SegmentState::Pending);
                        return Err(SiphonError::Cancelled);
                    }
                    _ = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {}
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn download_segment_once(
    seg: &Arc<LiveSegment>,
    supports_range: bool,
    client: &Client,
    url: &str,
    custom_headers: &reqwest::header::HeaderMap,
    if_range_val: Option<&str>,
    file: &std::fs::File,
    limiter: &siphon_core::HierarchicalLimiter,
    cancel: &CancellationToken,
) -> Result<()> {
    let (cur, end) = seg.current_and_end();
    if end != u64::MAX && cur > end {
        return Ok(());
    }

    let mut req_headers = custom_headers.clone();
    let sent_range = supports_range && end != u64::MAX;
    if sent_range {
        let range_header = format!("bytes={cur}-{end}");
        if let Ok(val) = HeaderValue::from_str(&range_header) {
            req_headers.insert(RANGE, val);
        }
        if cur > 0
            && let Some(tag) = if_range_val
            && let Ok(val) = HeaderValue::from_str(tag)
        {
            req_headers.insert(IF_RANGE, val);
        }
    }

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(SiphonError::Cancelled),
        r = client.get(url).headers(req_headers).send() => {
            r.map_err(|e| SiphonError::Network(e.to_string()))?
        }
    };

    let status = resp.status();
    if matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::GONE
    ) {
        return Err(SiphonError::LinkExpired(status.as_u16()));
    }

    if sent_range && cur > 0 && status == StatusCode::OK {
        // Server ignored Range / If-Range failed -> remote resource changed
        return Err(SiphonError::RemoteFileChanged);
    }

    if !status.is_success() {
        return Err(SiphonError::Network(format!(
            "分片请求失败 HTTP {}",
            status.as_u16()
        )));
    }

    let mut stream = resp.bytes_stream();
    const WRITE_BUF_CAPACITY: usize = 256 * 1024;
    let mut write_buf: Vec<u8> = Vec::with_capacity(WRITE_BUF_CAPACITY);
    let mut buf_start_offset: u64 = 0;

    let flush_buf = |buf: &mut Vec<u8>, start_off: u64| -> Result<()> {
        if !buf.is_empty() {
            write_all_at(file, start_off, buf)?;
            buf.clear();
        }
        Ok(())
    };

    loop {
        let next_chunk = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = flush_buf(&mut write_buf, buf_start_offset);
                return Err(SiphonError::Cancelled);
            }
            chunk = stream.next() => chunk,
        };

        let Some(chunk_res) = next_chunk else {
            break;
        };

        let chunk = match chunk_res {
            Ok(c) => c,
            Err(e) => {
                let _ = flush_buf(&mut write_buf, buf_start_offset);
                return Err(SiphonError::Network(e.to_string()));
            }
        };
        if chunk.is_empty() {
            continue;
        }

        let Some((write_offset, allowed, reached_end)) = seg.reserve_write(chunk.len()) else {
            // Segment boundary was shrunk by a work-stealing peer right up to our cursor!
            flush_buf(&mut write_buf, buf_start_offset)?;
            return Ok(());
        };

        if !limiter.consume(allowed as u64, cancel).await {
            let _ = flush_buf(&mut write_buf, buf_start_offset);
            return Err(SiphonError::Cancelled);
        }

        if write_buf.is_empty() {
            buf_start_offset = write_offset;
        } else if buf_start_offset + write_buf.len() as u64 != write_offset {
            flush_buf(&mut write_buf, buf_start_offset)?;
            buf_start_offset = write_offset;
        }

        write_buf.extend_from_slice(&chunk[..allowed]);
        seg.record_written(allowed as u64);

        if write_buf.len() >= WRITE_BUF_CAPACITY {
            flush_buf(&mut write_buf, buf_start_offset)?;
        }

        if reached_end {
            // Stop immediately — do not read past our possibly-shrunk dynamic boundary!
            flush_buf(&mut write_buf, buf_start_offset)?;
            return Ok(());
        }
    }

    flush_buf(&mut write_buf, buf_start_offset)?;

    // Verify we reached the end if length was known
    let (final_cur, final_end) = seg.current_and_end();
    if final_end != u64::MAX && final_cur <= final_end {
        return Err(SiphonError::Network(format!(
            "连接提前中断（已写 {} / 目标 {}）",
            final_cur,
            final_end + 1
        )));
    }

    Ok(())
}
