use async_trait::async_trait;
use chrono::Utc;
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::DhtPersistenceConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ConnectionOptions, DhtSessionConfig,
    ListenerMode, ListenerOptions, PeerConnectionOptions, Session, SessionOptions,
};
use percent_encoding::percent_decode_str;
use siphon_core::{
    BtConfig, DownloadContext, DownloadOutcome, EngineUpdate, FileCategory, ProbeMetadata,
    ProtocolHandler, ProtocolKind, Result, SegmentInfo, SegmentState, SiphonError, TaskLogEntry,
    parse_tracker_list,
};
use std::collections::{BTreeMap, HashSet};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionFingerprint {
    enable_dht: bool,
    enable_lsd: bool,
    enable_upnp: bool,
    disable_trackers: bool,
    listen_port: u16,
    peer_connect_timeout_secs: u64,
    max_peers_per_torrent: usize,
    proxy_url: Option<String>,
}

impl SessionFingerprint {
    fn from_cfg(bt: &BtConfig, proxy_url: Option<&str>) -> Self {
        Self {
            enable_dht: bt.enable_dht,
            enable_lsd: bt.enable_lsd,
            enable_upnp: bt.enable_upnp,
            disable_trackers: bt.disable_trackers,
            listen_port: bt.listen_port,
            peer_connect_timeout_secs: bt.peer_connect_timeout_secs,
            max_peers_per_torrent: bt.max_peers_per_torrent,
            proxy_url: proxy_url
                .map(str::trim)
                .filter(|p| p.to_ascii_lowercase().starts_with("socks5://"))
                .map(str::to_string),
        }
    }
}

struct WarmSession {
    fingerprint: SessionFingerprint,
    session: Arc<Session>,
}

/// Single sub-file entry inside a BitTorrent `.torrent` or Magnet metadata payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentFileEntry {
    pub index: usize,
    pub path: String,
    pub size: u64,
    pub is_padding: bool,
}

/// Parsed metadata and sub-file listing returned by `BitTorrentEngine::inspect_torrent`.
#[derive(Debug, Clone)]
pub struct TorrentInspectInfo {
    pub normalized_url: String,
    pub name: String,
    pub info_hash: String,
    pub total_size: u64,
    pub files: Vec<TorrentFileEntry>,
}

/// Pure-Rust BitTorrent / Magnet download engine powered by `librqbit`,
/// using a long-lived pre-warmed shared session with persisted DHT routing table,
/// TCP+uTP listeners, and full custom tracker injection for instant task startup.
pub struct BitTorrentEngine {
    data_dir: Option<PathBuf>,
    warm: Mutex<Option<WarmSession>>,
}

impl std::fmt::Debug for BitTorrentEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitTorrentEngine")
            .field("data_dir", &self.data_dir)
            .finish()
    }
}

impl Default for BitTorrentEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BitTorrentEngine {
    pub fn new() -> Self {
        Self {
            data_dir: None,
            warm: Mutex::new(None),
        }
    }

    /// Create a `BitTorrentEngine` that persists its DHT routing table under `data_dir`.
    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            data_dir: Some(data_dir),
            warm: Mutex::new(None),
        }
    }

    /// Pre-warm the shared BitTorrent session & DHT routing table in the background at app startup
    /// so subsequent Magnet / `.torrent` downloads attach in milliseconds instead of cold-bootstrapping.
    pub async fn warmup(&self, bt: &BtConfig, default_output_dir: &Path, proxy_url: Option<&str>) {
        if !bt.enable_dht && bt.disable_trackers {
            return;
        }
        let _ = self
            .get_or_create_session(bt, default_output_dir, proxy_url)
            .await;
    }

    async fn get_or_create_session(
        &self,
        bt: &BtConfig,
        fallback_dir: &Path,
        proxy_url: Option<&str>,
    ) -> Result<(Arc<Session>, bool)> {
        let fp = SessionFingerprint::from_cfg(bt, proxy_url);
        let mut guard = self.warm.lock().await;
        if let Some(existing) = guard.as_ref()
            && existing.fingerprint == fp
        {
            return Ok((Arc::clone(&existing.session), true));
        }

        // Replace old session cleanly if BT network settings changed
        if let Some(old) = guard.take() {
            old.session.stop().await;
        }

        let base_dir = self
            .data_dir
            .as_deref()
            .unwrap_or(fallback_dir)
            .to_path_buf();
        let _ = tokio::fs::create_dir_all(&base_dir).await;

        let dht_file = self.data_dir.as_ref().map(|d| d.join("bt-dht.json"));
        let opts = build_session_options_with_dht_path(bt, proxy_url, dht_file);

        let session = Session::new_with_opts(fallback_dir.to_path_buf(), opts)
            .await
            .map_err(|e| SiphonError::Network(format!("BitTorrent 会话初始化失败: {e}")))?;

        *guard = Some(WarmSession {
            fingerprint: fp,
            session: Arc::clone(&session),
        });
        Ok((session, false))
    }

    /// Inspect a `.torrent` file (local path, `file://`, `http(s)://`) or `magnet:?` link
    /// without starting the download, returning its full sub-file list for selective download.
    pub async fn inspect_torrent(
        &self,
        raw_input: &str,
        bt: &BtConfig,
        fallback_dir: &Path,
        proxy_url: Option<&str>,
    ) -> std::result::Result<TorrentInspectInfo, String> {
        let trimmed = raw_input.trim().trim_matches('"');
        if trimmed.is_empty() {
            return Err("请输入磁力链接、种子直链或选择本地 .torrent 文件".to_string());
        }

        // Normalize a bare local `.torrent` path into a `file:///` URL cleanly
        let (normalized_url, local_path) = if let Some(p) = local_torrent_path_from_url(trimmed) {
            (trimmed.to_string(), Some(p))
        } else {
            let candidate_path = PathBuf::from(trimmed);
            if candidate_path.is_file()
                && candidate_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("torrent"))
            {
                let file_url = Url::from_file_path(&candidate_path)
                    .map(|u| u.to_string())
                    .map_err(|_| format!("无法解析本地种子路径: {}", candidate_path.display()))?;
                (file_url, Some(candidate_path))
            } else {
                (trimmed.to_string(), None)
            }
        };

        if !is_bittorrent_url(&normalized_url) {
            return Err("仅支持 magnet:? 磁力链接、.torrent 种子链接或本地 .torrent 文件".to_string());
        }

        let add_req = if let Some(path) = local_path {
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| format!("读取本地种子文件失败 ({}): {e}", path.display()))?;
            AddTorrent::from_bytes(bytes)
        } else {
            AddTorrent::from_url(&normalized_url)
        };

        let (session, _reused) = self
            .get_or_create_session(bt, fallback_dir, proxy_url)
            .await
            .map_err(|e| e.to_string())?;

        let mut opts = build_add_torrent_options_for_dir(bt, Some(fallback_dir));
        opts.list_only = true;

        let response = tokio::time::timeout(
            Duration::from_secs(45),
            session.add_torrent(add_req, Some(opts)),
        )
        .await
        .map_err(|_| {
            "获取磁力链接元数据超时（45 秒内暂未找到持有元数据的 Peer，可直接点击「不等待列表直接下载」）"
                .to_string()
        })?
        .map_err(|e| format!("解析种子/磁力元数据失败: {e}"))?;

        match response {
            AddTorrentResponse::ListOnly(lo) => {
                let info_hash = lo.info_hash.as_string();
                let name = lo
                    .info
                    .name()
                    .map(|n| sanitize_bt_name(&n))
                    .unwrap_or_else(|| format!("torrent-{}", &info_hash[..info_hash.len().min(12)]));
                let mut files = Vec::new();
                let mut total_size = 0u64;
                for (index, details) in lo.info.iter_file_details().enumerate() {
                    let is_padding = details.attrs().padding;
                    let size = details.len;
                    if !is_padding {
                        total_size = total_size.saturating_add(size);
                    }
                    files.push(TorrentFileEntry {
                        index,
                        path: details.filename.to_string(),
                        size,
                        is_padding,
                    });
                }
                Ok(TorrentInspectInfo {
                    normalized_url,
                    name,
                    info_hash,
                    total_size,
                    files,
                })
            }
            AddTorrentResponse::AlreadyManaged(_, handle)
            | AddTorrentResponse::Added(_, handle) => {
                let info_hash = handle.info_hash().as_string();
                let name = handle
                    .name()
                    .map(|n| sanitize_bt_name(&n))
                    .unwrap_or_else(|| format!("torrent-{}", &info_hash[..info_hash.len().min(12)]));
                let files = handle
                    .with_metadata(|meta| {
                        let mut list = Vec::new();
                        for (index, details) in meta.info.iter_file_details().enumerate() {
                            list.push(TorrentFileEntry {
                                index,
                                path: details.filename.to_string(),
                                size: details.len,
                                is_padding: details.attrs().padding,
                            });
                        }
                        list
                    })
                    .unwrap_or_default();
                let total_size = files
                    .iter()
                    .filter(|f| !f.is_padding)
                    .map(|f| f.size)
                    .sum();
                Ok(TorrentInspectInfo {
                    normalized_url,
                    name,
                    info_hash,
                    total_size,
                    files,
                })
            }
        }
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
    build_session_options_with_dht_path(bt, proxy_url, None)
}

pub fn build_session_options_with_dht_path(
    bt: &BtConfig,
    proxy_url: Option<&str>,
    dht_config_path: Option<PathBuf>,
) -> SessionOptions {
    let dht = if bt.enable_dht {
        let persistence = dht_config_path.map(|path| DhtPersistenceConfig {
            dump_interval: Some(Duration::from_secs(30)),
            config_filename: Some(path),
        });
        Some(DhtSessionConfig {
            bootstrap_addrs: Some(vec![
                "router.bittorrent.com:6881".to_string(),
                "router.utorrent.com:6881".to_string(),
                "dht.transmissionbt.com:6881".to_string(),
                "dht.aelitis.com:6881".to_string(),
                "dht.libtorrent.org:25401".to_string(),
            ]),
            port: if bt.listen_port > 0 {
                Some(bt.listen_port)
            } else {
                None
            },
            persistence,
        })
    } else {
        None
    };

    let listen = Some(ListenerOptions {
        mode: ListenerMode::TcpAndUtp,
        listen_addr: (Ipv4Addr::UNSPECIFIED, bt.listen_port).into(),
        enable_upnp_port_forwarding: bt.enable_upnp,
        ipv4_only: true,
        ..Default::default()
    });

    let socks_proxy = proxy_url
        .map(str::trim)
        .filter(|p| p.to_ascii_lowercase().starts_with("socks5://"))
        .map(str::to_string);

    let connect = Some(ConnectionOptions {
        enable_tcp: true,
        proxy_url: socks_proxy,
        peer_opts: Some(PeerConnectionOptions {
            connect_timeout: Some(Duration::from_secs(bt.peer_connect_timeout_secs.clamp(2, 60))),
            read_write_timeout: Some(Duration::from_secs(12)),
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
        ipv4_only: true,
        client_name_and_version: Some(format!("Siphon/{}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    }
}

/// Build `librqbit::AddTorrentOptions` with per-task output folder and custom/synced trackers injected.
pub fn build_add_torrent_options(bt: &BtConfig) -> AddTorrentOptions {
    build_add_torrent_options_for_dir(bt, None)
}

pub fn build_add_torrent_options_for_dir(
    bt: &BtConfig,
    output_dir: Option<&Path>,
) -> AddTorrentOptions {
    build_add_torrent_options_with_files(bt, output_dir, None)
}

pub fn build_add_torrent_options_with_files(
    bt: &BtConfig,
    output_dir: Option<&Path>,
    only_files: Option<&[usize]>,
) -> AddTorrentOptions {
    let trackers = if bt.auto_append_trackers && !bt.disable_trackers {
        let list = bt.effective_trackers();
        if list.is_empty() { None } else { Some(list) }
    } else {
        None
    };

    let only_files_opt = only_files
        .filter(|list| !list.is_empty())
        .map(|list| list.to_vec());

    AddTorrentOptions {
        overwrite: true,
        output_folder: output_dir.map(|p| p.to_string_lossy().to_string()),
        only_files: only_files_opt,
        disable_trackers: bt.disable_trackers,
        peer_limit: Some(bt.max_peers_per_torrent.clamp(10, 500)),
        force_tracker_interval: Some(Duration::from_secs(60)),
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
        let t0 = Instant::now();
        let output_dir = ctx.task.save_dir.clone();
        tokio::fs::create_dir_all(&output_dir).await?;

        let (session, reused) = self
            .get_or_create_session(&ctx.bt_config, &output_dir, ctx.proxy_url.as_deref())
            .await?;

        let add_opts = build_add_torrent_options_with_files(
            &ctx.bt_config,
            Some(&output_dir),
            ctx.task.selected_files.as_deref(),
        );
        let tracker_count = add_opts.trackers.as_ref().map(|v| v.len()).unwrap_or(0);
        let selected_note = match ctx.task.selected_files.as_ref() {
            Some(files) if !files.is_empty() => format!("，仅下载勾选的 {} 个子文件", files.len()),
            _ => String::new(),
        };
        push_log(
            &ctx,
            "info",
            format!(
                "BitTorrent 会话{}（{} ms，TCP+uTP，DHT={}，注入 Tracker={} 条{}）-> {}",
                if reused { "热复用" } else { "已初始化" },
                t0.elapsed().as_millis(),
                ctx.bt_config.enable_dht,
                tracker_count,
                selected_note,
                output_dir.display()
            ),
        );

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

        let response = tokio::select! {
            _ = ctx.cancel.cancelled() => {
                return Err(SiphonError::Cancelled);
            }
            res = session.add_torrent(add_req, Some(add_opts)) => {
                res.map_err(|e| SiphonError::Network(format!("解析磁力/种子失败: {e}")))?
            }
        };

        let (torrent_id, handle) = match response {
            AddTorrentResponse::Added(id, h) | AddTorrentResponse::AlreadyManaged(id, h) => {
                (id, h)
            }
            AddTorrentResponse::ListOnly(_) => {
                return Err(SiphonError::Network("种子仅处于列表模式".into()));
            }
        };

        push_log(
            &ctx,
            "info",
            format!(
                "已加入 BitTorrent Swarm（耗时 {} ms），正在并发探测元数据与 Peer...",
                t0.elapsed().as_millis()
            ),
        );

        let mut metadata_emitted = false;
        let mut last_peer_summary = (0u32, 0u32);

        loop {
            if ctx.cancel.is_cancelled() {
                let _ = session.delete(TorrentIdOrHash::Id(torrent_id), false).await;
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
                        "元数据就绪: {} ({} bytes，总初始化 {:.1}s)",
                        resolved_name,
                        stats.total_bytes,
                        t0.elapsed().as_secs_f32()
                    ),
                );
            }

            let (speed_bps, eta_secs, live_peers, seen_peers) =
                if let Some(live) = stats.live.as_ref() {
                    let bps = (live.download_speed.mbps * 1024.0 * 1024.0).max(0.0) as u64;
                    let remaining = stats.total_bytes.saturating_sub(stats.progress_bytes);
                    let eta = if bps > 0 && stats.total_bytes > 0 {
                        Some(remaining / bps)
                    } else {
                        None
                    };
                    let ps = &live.snapshot.peer_stats;
                    (bps, eta, ps.live, ps.seen)
                } else {
                    (0, None, 0, 0)
                };

            if (live_peers, seen_peers) != last_peer_summary && (live_peers > 0 || seen_peers > 0) {
                last_peer_summary = (live_peers, seen_peers);
                push_log(
                    &ctx,
                    "info",
                    format!("Peer 状态：活跃连接 {live_peers} · 已发现 {seen_peers}"),
                );
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
                let _ = session.delete(TorrentIdOrHash::Id(torrent_id), false).await;
                return Err(SiphonError::Network(format!("BitTorrent 传输错误: {err}")));
            }

            if stats.finished {
                let total_bytes = stats.total_bytes.max(stats.progress_bytes);
                push_log(&ctx, "info", "BitTorrent 全部数据校验并下载完成");
                let _ = session.delete(TorrentIdOrHash::Id(torrent_id), false).await;
                return Ok(DownloadOutcome {
                    total_bytes,
                    verified_hash: None,
                });
            }

            tokio::select! {
                _ = ctx.cancel.cancelled() => {
                    let _ = session.delete(TorrentIdOrHash::Id(torrent_id), false).await;
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
        assert!(!engine.supports_url(
            "ed2k://|file|test.iso|1024|31D6CFE0D16AE931B73C59D7E0C089C0|/"
        ));
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
            peer_connect_timeout_secs: 5,
            max_peers_per_torrent: 200,
            auto_append_trackers: true,
            custom_trackers: vec![
                "udp://tracker.example.org:1337/announce".into(),
                "https://tracker.example.net/announce".into(),
            ],
            synced_trackers: vec![
                "udp://tracker.example.org:1337/announce".into(),
                "udp://synced.example.com:80/announce".into(),
            ],
            ..Default::default()
        };

        let sess = build_session_options(&bt, Some("socks5://127.0.0.1:1080"));
        assert!(sess.dht.is_none());
        assert!(sess.disable_local_service_discovery);
        assert!(sess.ipv4_only);
        assert_eq!(sess.peer_limit, Some(200));
        assert_eq!(sess.trackers.len(), 3);
        let listen = sess.listen.expect("listen options present");
        assert_eq!(listen.listen_addr.port(), 6889);
        assert!(listen.enable_upnp_port_forwarding);
        assert_eq!(
            sess.connect.as_ref().and_then(|c| c.proxy_url.as_deref()),
            Some("socks5://127.0.0.1:1080")
        );

        let add = build_add_torrent_options_for_dir(&bt, Some(Path::new(r"D:\Downloads")));
        assert_eq!(add.output_folder.as_deref(), Some(r"D:\Downloads"));
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

    #[tokio::test]
    async fn warm_session_is_reused_across_calls_and_rebuilt_on_config_change() {
        let dir = tempfile::tempdir().unwrap();
        let engine = BitTorrentEngine::with_data_dir(dir.path().to_path_buf());
        let mut bt = BtConfig {
            enable_dht: false,
            enable_lsd: false,
            enable_upnp: false,
            disable_trackers: true,
            listen_port: 0,
            peer_connect_timeout_secs: 3,
            max_peers_per_torrent: 40,
            auto_append_trackers: false,
            ..Default::default()
        };

        let (s1, reused1) = engine
            .get_or_create_session(&bt, dir.path(), None)
            .await
            .unwrap();
        assert!(!reused1);

        let (s2, reused2) = engine
            .get_or_create_session(&bt, dir.path(), None)
            .await
            .unwrap();
        assert!(reused2);
        assert!(Arc::ptr_eq(&s1, &s2));

        // Changing peer limit invalidates fingerprint and replaces the warm session cleanly
        bt.max_peers_per_torrent = 80;
        let (s3, reused3) = engine
            .get_or_create_session(&bt, dir.path(), None)
            .await
            .unwrap();
        assert!(!reused3);
        assert!(!Arc::ptr_eq(&s1, &s3));

        s3.stop().await;
    }

    #[tokio::test]
    async fn inspect_local_multi_file_torrent_lists_all_subfiles_and_only_files_options() {
        let dir = tempfile::tempdir().unwrap();
        // Build a valid bencoded multi-file `.torrent` with 3 sub-files (total 3000 bytes, 1 piece of 16384 bytes)
        let mut torrent_bytes: Vec<u8> = Vec::new();
        torrent_bytes.extend_from_slice(b"d4:infod5:filesl");
        torrent_bytes.extend_from_slice(b"d6:lengthi1500e4:pathl7:Season18:EP01.mkvee");
        torrent_bytes.extend_from_slice(b"d6:lengthi1200e4:pathl7:Season18:EP02.mkvee");
        torrent_bytes.extend_from_slice(b"d6:lengthi300e4:pathl10:README.txtee");
        torrent_bytes.extend_from_slice(b"e4:name12:SampleSeries12:piece lengthi16384e6:pieces20:");
        torrent_bytes.extend_from_slice(&[0xABu8; 20]);
        torrent_bytes.extend_from_slice(b"ee");

        let torrent_path = dir.path().join("sample_series.torrent");
        std::fs::write(&torrent_path, &torrent_bytes).unwrap();

        let engine = BitTorrentEngine::with_data_dir(dir.path().to_path_buf());
        let bt = BtConfig {
            enable_dht: false,
            enable_lsd: false,
            enable_upnp: false,
            disable_trackers: true,
            listen_port: 0,
            auto_append_trackers: false,
            ..Default::default()
        };

        let info = engine
            .inspect_torrent(
                &torrent_path.to_string_lossy(),
                &bt,
                dir.path(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(info.name, "SampleSeries");
        assert_eq!(info.total_size, 3000);
        assert_eq!(info.files.len(), 3);
        assert!(info.normalized_url.starts_with("file:///"));
        assert_eq!(info.files[0].index, 0);
        assert_eq!(info.files[0].size, 1500);
        assert!(info.files[0].path.contains("EP01.mkv"));
        assert_eq!(info.files[1].size, 1200);
        assert_eq!(info.files[2].size, 300);

        let add = build_add_torrent_options_with_files(&bt, Some(dir.path()), Some(&[0, 1]));
        assert_eq!(add.only_files, Some(vec![0, 1]));
    }
}
