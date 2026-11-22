use crate::playlist::{HlsKeySpec, HlsMediaPlaylist, ParsedPlaylist, parse_m3u8};
use aes::Aes128;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut, KeyIvInit};
use chrono::Utc;
use reqwest::Client;
use siphon_core::{
    DownloadContext, DownloadOutcome, EngineUpdate, FileCategory, ProbeMetadata, Result,
    SegmentInfo, SegmentState, SiphonError, TaskLogEntry, deduplicate_filename, format_bytes,
    infer_filename_from_url, sanitize_filename,
};
use siphon_engine_http::probe::{build_client, build_header_map};
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use url::Url;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

fn send_log(tx: &mpsc::UnboundedSender<EngineUpdate>, level: &str, msg: impl Into<String>) {
    let _ = tx.send(EngineUpdate::Log(TaskLogEntry {
        timestamp: Utc::now(),
        level: level.to_string(),
        message: msg.into(),
    }));
}

async fn fetch_text(
    client: &Client,
    url: &str,
    headers: &reqwest::header::HeaderMap,
) -> Result<(Url, String)> {
    let resp = client
        .get(url)
        .headers(headers.clone())
        .send()
        .await
        .map_err(|e| SiphonError::Network(format!("请求 M3U8 失败: {e}")))?;
    let status = resp.status();
    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED
            | reqwest::StatusCode::FORBIDDEN
            | reqwest::StatusCode::GONE
    ) {
        return Err(SiphonError::LinkExpired(status.as_u16()));
    }
    if !status.is_success() {
        return Err(SiphonError::Network(format!(
            "获取 M3U8 播放列表失败 HTTP {}",
            status.as_u16()
        )));
    }
    let final_url = resp.url().clone();
    let text = resp
        .text()
        .await
        .map_err(|e| SiphonError::Network(format!("读取 M3U8 内容失败: {e}")))?;
    Ok((final_url, text))
}

pub async fn resolve_media_playlist(
    client: &Client,
    raw_url: &str,
    headers: &reqwest::header::HeaderMap,
) -> Result<(Url, HlsMediaPlaylist, Option<usize>)> {
    let (mut cur_url, text) = fetch_text(client, raw_url, headers).await?;
    match parse_m3u8(&cur_url, &text)? {
        ParsedPlaylist::Media(media) => Ok((cur_url, media, None)),
        ParsedPlaylist::Master {
            best_variant_url,
            variants_count,
        } => {
            let (variant_url, variant_text) = fetch_text(client, &best_variant_url, headers).await?;
            cur_url = variant_url;
            match parse_m3u8(&cur_url, &variant_text)? {
                ParsedPlaylist::Media(media) => Ok((cur_url, media, Some(variants_count))),
                ParsedPlaylist::Master { .. } => Err(SiphonError::Network(
                    "HLS 播放列表嵌套层数过多".to_string(),
                )),
            }
        }
    }
}

pub async fn probe_hls(
    raw_url: &str,
    headers: &BTreeMap<String, String>,
    proxy_url: Option<&str>,
    user_agent: &str,
) -> Result<ProbeMetadata> {
    let client = build_client(proxy_url, user_agent)?;
    let hdr_map = build_header_map(headers);
    let (final_url, playlist, _) = resolve_media_playlist(&client, raw_url, &hdr_map).await?;

    let mut filename = sanitize_filename(&infer_filename_from_url(&final_url));
    if playlist.is_fmp4 && filename.to_ascii_lowercase().ends_with(".ts") {
        let len = filename.len();
        filename.replace_range(len - 3..len, ".mp4");
    }

    Ok(ProbeMetadata {
        final_url: final_url.to_string(),
        filename,
        content_length: None,
        content_type: Some(if playlist.is_fmp4 {
            "video/mp4".to_string()
        } else {
            "video/mp2t".to_string()
        }),
        supports_range: true,
        etag: None,
        last_modified: None,
        suggested_category: FileCategory::Video,
        resolved_headers: BTreeMap::new(),
        plugin_note: Some(format!(
            "HLS {} 分片 · 时长 {:.0}s",
            playlist.segments.len(),
            playlist.total_duration_secs
        )),
    })
}

pub async fn run_hls_download(ctx: DownloadContext) -> Result<DownloadOutcome> {
    let task = &ctx.task;
    let cancel = ctx.cancel.clone();
    let client = build_client(ctx.proxy_url.as_deref(), &ctx.user_agent)?;
    let headers = Arc::new(build_header_map(&task.headers));

    send_log(
        &ctx.updates_tx,
        "INFO",
        format!("开始解析 HLS 播放列表: {}", task.resolved_url),
    );

    let (final_url, playlist, variant_count) = tokio::select! {
        _ = cancel.cancelled() => return Err(SiphonError::Cancelled),
        res = resolve_media_playlist(&client, &task.resolved_url, &headers) => res?,
    };

    if let Some(vc) = variant_count {
        send_log(
            &ctx.updates_tx,
            "INFO",
            format!("已从 Master Playlist ({vc} 个画质) 自动选择最高码率流"),
        );
    }

    let seg_total = playlist.segments.len();
    let ext = if playlist.is_fmp4 { ".mp4" } else { ".ts" };
    let mut final_filename = task.filename.clone();
    if !final_filename.to_ascii_lowercase().ends_with(".mp4")
        && !final_filename.to_ascii_lowercase().ends_with(".ts")
    {
        final_filename.push_str(ext);
        final_filename = deduplicate_filename(&task.save_dir, &final_filename);
    } else if playlist.is_fmp4 && final_filename.to_ascii_lowercase().ends_with(".ts") {
        let len = final_filename.len();
        final_filename.replace_range(len - 3..len, ".mp4");
    }

    let _ = ctx.updates_tx.send(EngineUpdate::Probed(ProbeMetadata {
        final_url: final_url.to_string(),
        filename: final_filename.clone(),
        content_length: None,
        content_type: Some(if playlist.is_fmp4 {
            "video/mp4".to_string()
        } else {
            "video/mp2t".to_string()
        }),
        supports_range: true,
        etag: None,
        last_modified: None,
        suggested_category: FileCategory::Video,
        resolved_headers: BTreeMap::new(),
        plugin_note: None,
    }));

    send_log(
        &ctx.updates_tx,
        "INFO",
        format!(
            "HLS 解析完成: 共 {seg_total} 个分片，总时长 {:.1}s，开始多线程并行拉取",
            playlist.total_duration_secs
        ),
    );

    let target_path = task.save_dir.join(&final_filename);
    let temp_dir = task.save_dir.join(format!("{final_filename}.siphon.hls"));
    std::fs::create_dir_all(&temp_dir)?;

    // Optional fMP4 init segment
    let init_path = if let Some(init_url) = &playlist.init_map_url {
        let p = temp_dir.join("init.bin");
        if !p.is_file() {
            let bytes = fetch_bytes_with_retries(&client, init_url, &headers, &cancel).await?;
            std::fs::write(&p, &bytes)?;
        }
        Some(p)
    } else {
        None
    };

    // Shared state across parallel segment workers
    let next_idx = Arc::new(AtomicUsize::new(0));
    let done_count = Arc::new(AtomicUsize::new(0));
    let total_downloaded = Arc::new(AtomicU64::new(0));
    let window_bytes = Arc::new(AtomicU64::new(0));
    let seg_states = Arc::new(Mutex::new(vec![SegmentState::Pending; seg_total]));
    let key_cache: Arc<Mutex<HashMap<String, [u8; 16]>>> = Arc::new(Mutex::new(HashMap::new()));
    let segments = Arc::new(playlist.segments);

    // Pre-scan already completed segments for instant resume support
    {
        let mut states = seg_states.lock().await;
        for i in 0..seg_total {
            let seg_path = temp_dir.join(format!("seg_{i:06}.bin"));
            if let Ok(meta) = std::fs::metadata(&seg_path)
                && meta.len() > 0
            {
                states[i] = SegmentState::Done;
                done_count.fetch_add(1, Ordering::Relaxed);
                total_downloaded.fetch_add(meta.len(), Ordering::Relaxed);
            }
        }
    }

    // Periodic progress emitter (250ms) mapping HLS segments onto Siphon's live speed & segment bar
    let rep_done = Arc::clone(&done_count);
    let rep_dl = Arc::clone(&total_downloaded);
    let rep_win = Arc::clone(&window_bytes);
    let rep_states = Arc::clone(&seg_states);
    let rep_tx = ctx.updates_tx.clone();
    let rep_cancel = CancellationToken::new();
    let rep_stop = rep_cancel.clone();

    let reporter_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        let mut last_tick = Instant::now();
        let mut smoothed_speed: f64 = 0.0;

        loop {
            tokio::select! {
                _ = rep_stop.cancelled() => break,
                _ = interval.tick() => {
                    let now = Instant::now();
                    let dt = now.duration_since(last_tick).as_secs_f64().max(0.05);
                    last_tick = now;

                    let bytes_now = rep_win.swap(0, Ordering::Relaxed);
                    let inst_spd = (bytes_now as f64 / dt).max(0.0);
                    smoothed_speed = if smoothed_speed <= 1.0 {
                        inst_spd
                    } else {
                        smoothed_speed * 0.65 + inst_spd * 0.35
                    };
                    let speed_bps = smoothed_speed.round() as u64;
                    let dl = rep_dl.load(Ordering::Relaxed);
                    let done = rep_done.load(Ordering::Relaxed);

                    let est_total = if done > 0 && done < seg_total {
                        Some(((dl as f64 / done as f64) * seg_total as f64).round() as u64)
                    } else if done >= seg_total && dl > 0 {
                        Some(dl)
                    } else {
                        None
                    };
                    let eta_secs = match (est_total, speed_bps) {
                        (Some(tot), spd) if spd > 0 && tot > dl => Some((tot - dl) / spd),
                        (Some(tot), _) if tot <= dl => Some(0),
                        _ => None,
                    };

                    // Bucket segment states into at most 32 clean visual segments for SegmentBar
                    let states = rep_states.lock().await;
                    let visual_segs = build_visual_segments(&states, est_total.unwrap_or(seg_total as u64));
                    drop(states);

                    let _ = rep_tx.send(EngineUpdate::Progress {
                        downloaded_bytes: dl,
                        total_size: est_total,
                        speed_bps,
                        eta_secs,
                        segments: visual_segs,
                    });
                }
            }
        }
    });

    let workers_count = ctx
        .live_max_connections
        .load(Ordering::Relaxed)
        .clamp(1, 16) as usize;
    let workers_cancel = cancel.child_token();
    let mut handles = Vec::with_capacity(workers_count);

    for _ in 0..workers_count {
        let next_idx = Arc::clone(&next_idx);
        let done_count = Arc::clone(&done_count);
        let total_dl = Arc::clone(&total_downloaded);
        let win_bytes = Arc::clone(&window_bytes);
        let states = Arc::clone(&seg_states);
        let keys = Arc::clone(&key_cache);
        let segs = Arc::clone(&segments);
        let client = client.clone();
        let hdrs = Arc::clone(&headers);
        let tdir = temp_dir.clone();
        let limiter = ctx.limiter.clone();
        let w_cancel = workers_cancel.clone();

        handles.push(tokio::spawn(async move {
            loop {
                if w_cancel.is_cancelled() {
                    return Err(SiphonError::Cancelled);
                }
                let idx = next_idx.fetch_add(1, Ordering::SeqCst);
                if idx >= segs.len() {
                    return Ok(());
                }

                // Skip already-downloaded segment on resume
                {
                    let st = states.lock().await;
                    if st[idx] == SegmentState::Done {
                        continue;
                    }
                }
                {
                    let mut st = states.lock().await;
                    st[idx] = SegmentState::Active;
                }

                let seg = &segs[idx];
                let mut raw_bytes =
                    match fetch_bytes_with_retries(&client, &seg.url, &hdrs, &w_cancel).await {
                        Ok(b) => b,
                        Err(e) => {
                            let mut st = states.lock().await;
                            st[idx] = if matches!(e, SiphonError::Cancelled) {
                                SegmentState::Pending
                            } else {
                                SegmentState::Error
                            };
                            return Err(e);
                        }
                    };

                if !limiter.consume(raw_bytes.len() as u64, &w_cancel).await {
                    let mut st = states.lock().await;
                    st[idx] = SegmentState::Pending;
                    return Err(SiphonError::Cancelled);
                }

                if let Some(key_spec) = &seg.key {
                    let key_bytes =
                        fetch_or_cached_key(&client, &hdrs, key_spec, &keys, &w_cancel).await?;
                    let iv = key_spec.iv.unwrap_or_else(|| {
                        let mut b = [0u8; 16];
                        b[8..16].copy_from_slice(&seg.sequence.to_be_bytes());
                        b
                    });
                    raw_bytes = decrypt_aes128_cbc(&raw_bytes, &key_bytes, &iv)?;
                }

                let part_file = tdir.join(format!("seg_{idx:06}.part"));
                let done_file = tdir.join(format!("seg_{idx:06}.bin"));
                std::fs::write(&part_file, &raw_bytes)?;
                let _ = std::fs::remove_file(&done_file);
                std::fs::rename(&part_file, &done_file)?;

                let len = raw_bytes.len() as u64;
                total_dl.fetch_add(len, Ordering::Relaxed);
                win_bytes.fetch_add(len, Ordering::Relaxed);
                done_count.fetch_add(1, Ordering::Relaxed);
                {
                    let mut st = states.lock().await;
                    st[idx] = SegmentState::Done;
                }
            }
        }));
    }

    let mut first_err: Option<SiphonError> = None;
    for h in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(SiphonError::Cancelled)) => {
                if first_err.is_none() {
                    first_err = Some(SiphonError::Cancelled);
                }
            }
            Ok(Err(e)) => {
                workers_cancel.cancel();
                if first_err.is_none() || matches!(first_err, Some(SiphonError::Cancelled)) {
                    first_err = Some(e);
                }
            }
            Err(je) => {
                workers_cancel.cancel();
                if first_err.is_none() {
                    first_err = Some(SiphonError::Network(format!("HLS 协程异常: {je}")));
                }
            }
        }
    }

    rep_cancel.cancel();
    let _ = reporter_handle.await;

    if let Some(err) = first_err {
        return Err(err);
    }

    // Merge all ordered segments into the final video file
    let _ = ctx.updates_tx.send(EngineUpdate::Verifying);
    send_log(
        &ctx.updates_tx,
        "INFO",
        format!("所有 {seg_total} 个 HLS 分片已拉取完成，正在顺序合并封装至 {final_filename}..."),
    );

    let merged_bytes = merge_hls_segments(&init_path, &temp_dir, seg_total, &target_path)?;
    let _ = std::fs::remove_dir_all(&temp_dir);

    let final_visual = vec![SegmentInfo {
        index: 0,
        start_offset: 0,
        current_offset: merged_bytes,
        end_offset: merged_bytes.saturating_sub(1),
        state: SegmentState::Done,
        speed_bps: 0,
    }];
    let _ = ctx.updates_tx.send(EngineUpdate::Progress {
        downloaded_bytes: merged_bytes,
        total_size: Some(merged_bytes),
        speed_bps: 0,
        eta_secs: Some(0),
        segments: final_visual,
    });

    send_log(
        &ctx.updates_tx,
        "INFO",
        format!("HLS 视频合并完成: {} ({})", final_filename, format_bytes(merged_bytes)),
    );

    Ok(DownloadOutcome {
        total_bytes: merged_bytes,
        verified_hash: None,
    })
}

fn build_visual_segments(states: &[SegmentState], est_total: u64) -> Vec<SegmentInfo> {
    if states.is_empty() || est_total == 0 {
        return Vec::new();
    }
    let buckets = states.len().min(32);
    let total = est_total.max(buckets as u64);
    let slot_bytes = total / buckets as u64;
    let mut out = Vec::with_capacity(buckets);

    for b in 0..buckets {
        let s_start = b * states.len() / buckets;
        let s_end = ((b + 1) * states.len() / buckets).max(s_start + 1).min(states.len());
        let slice = &states[s_start..s_end];
        let done = slice.iter().filter(|&&s| s == SegmentState::Done).count();
        let active = slice.iter().any(|&s| s == SegmentState::Active);
        let err = slice.iter().any(|&s| s == SegmentState::Error);

        let byte_start = b as u64 * slot_bytes;
        let byte_end = if b + 1 == buckets {
            total - 1
        } else {
            (b as u64 + 1) * slot_bytes - 1
        };
        let slot_len = byte_end - byte_start + 1;

        let (state, cur) = if done == slice.len() {
            (SegmentState::Done, byte_end + 1)
        } else if err {
            (SegmentState::Error, byte_start + (slot_len * done as u64 / slice.len() as u64))
        } else if active || done > 0 {
            (
                SegmentState::Active,
                byte_start + (slot_len * done as u64 / slice.len() as u64).max(1),
            )
        } else {
            (SegmentState::Pending, byte_start)
        };

        out.push(SegmentInfo {
            index: b as u32,
            start_offset: byte_start,
            current_offset: cur,
            end_offset: byte_end,
            state,
            speed_bps: 0,
        });
    }
    out
}

async fn fetch_bytes_with_retries(
    client: &Client,
    url: &str,
    headers: &reqwest::header::HeaderMap,
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    let mut attempt = 0u32;
    loop {
        if cancel.is_cancelled() {
            return Err(SiphonError::Cancelled);
        }
        let res = tokio::select! {
            _ = cancel.cancelled() => return Err(SiphonError::Cancelled),
            r = async {
                let resp = client.get(url).headers(headers.clone()).send().await
                    .map_err(|e| SiphonError::Network(e.to_string()))?;
                let st = resp.status();
                if matches!(st, reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::GONE) {
                    return Err(SiphonError::LinkExpired(st.as_u16()));
                }
                if !st.is_success() {
                    return Err(SiphonError::Network(format!("分片请求失败 HTTP {}", st.as_u16())));
                }
                let bytes = resp.bytes().await.map_err(|e| SiphonError::Network(e.to_string()))?;
                Ok::<Vec<u8>, SiphonError>(bytes.to_vec())
            } => r,
        };
        match res {
            Ok(b) => return Ok(b),
            Err(e @ SiphonError::Cancelled) | Err(e @ SiphonError::LinkExpired(_)) => return Err(e),
            Err(e) => {
                attempt += 1;
                if attempt > 4 {
                    return Err(e);
                }
                let wait = Duration::from_millis(400u64 << (attempt - 1));
                tokio::select! {
                    _ = cancel.cancelled() => return Err(SiphonError::Cancelled),
                    _ = tokio::time::sleep(wait) => {}
                }
            }
        }
    }
}

async fn fetch_or_cached_key(
    client: &Client,
    headers: &reqwest::header::HeaderMap,
    spec: &HlsKeySpec,
    cache: &Arc<Mutex<HashMap<String, [u8; 16]>>>,
    cancel: &CancellationToken,
) -> Result<[u8; 16]> {
    {
        let map = cache.lock().await;
        if let Some(k) = map.get(&spec.uri) {
            return Ok(*k);
        }
    }
    let raw = fetch_bytes_with_retries(client, &spec.uri, headers, cancel).await?;
    if raw.len() != 16 {
        return Err(SiphonError::Network(format!(
            "HLS AES-128 密钥长度异常（期望 16 字节，实际 {} 字节）",
            raw.len()
        )));
    }
    let mut key = [0u8; 16];
    key.copy_from_slice(&raw);
    cache.lock().await.insert(spec.uri.clone(), key);
    Ok(key)
}

fn decrypt_aes128_cbc(ciphertext: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>> {
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
        return Err(SiphonError::Network(
            "AES-128-CBC 加密分片长度不是 16 的倍数".to_string(),
        ));
    }
    let mut buf = ciphertext.to_vec();
    let dec = Aes128CbcDec::new_from_slices(key, iv)
        .map_err(|e| SiphonError::Network(format!("初始化 AES-128 解密器失败: {e}")))?;
    let plain = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| SiphonError::Network(format!("AES-128 分片解密失败: {e}")))?;
    Ok(plain.to_vec())
}

fn merge_hls_segments(
    init_path: &Option<PathBuf>,
    temp_dir: &PathBuf,
    seg_total: usize,
    target_path: &PathBuf,
) -> Result<u64> {
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let staging = target_path.with_extension("siphon.merging");
    let mut out = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&staging)?;
    let mut total = 0u64;
    let mut copy_buf = vec![0u8; 256 * 1024];

    let mut append_file = |src_path: &PathBuf| -> Result<()> {
        let mut f = File::open(src_path)?;
        loop {
            let n = f.read(&mut copy_buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&copy_buf[..n])?;
            total += n as u64;
        }
        Ok(())
    };

    if let Some(init) = init_path {
        append_file(init)?;
    }
    for i in 0..seg_total {
        let seg_file = temp_dir.join(format!("seg_{i:06}.bin"));
        append_file(&seg_file)?;
    }
    out.flush()?;
    drop(out);

    if target_path.exists() {
        let _ = std::fs::remove_file(target_path);
    }
    std::fs::rename(&staging, target_path)?;
    Ok(total)
}
