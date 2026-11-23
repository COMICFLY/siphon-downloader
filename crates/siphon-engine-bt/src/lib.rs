use async_trait::async_trait;
use chrono::Utc;
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::DhtPersistenceConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ConnectionOptions, DhtSessionConfig,
    EncryptionMode, ListenerMode, ListenerOptions, PeerConnectionOptions, Session, SessionOptions,
};
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use percent_encoding::percent_decode_str;
use siphon_core::{
    BtConfig, BtEncryptionMode, BtLiveTelemetry, BtPeerBrief, DownloadContext, DownloadOutcome,
    EngineUpdate, FileCategory, ProbeMetadata, ProtocolHandler, ProtocolKind, Result, SegmentInfo,
    SegmentState, SiphonError, TaskLogEntry, parse_tracker_list,
};
use std::collections::{BTreeMap, HashSet};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionFingerprint {
    enable_dht: bool,
    enable_lsd: bool,
    enable_upnp: bool,
    disable_trackers: bool,
    ipv4_only: bool,
    encryption_mode: BtEncryptionMode,
    listen_port: u16,
    peer_connect_timeout_secs: u64,
    peer_read_write_timeout_secs: u64,
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
            ipv4_only: bt.ipv4_only,
            encryption_mode: bt.encryption_mode,
            listen_port: bt.listen_port,
            peer_connect_timeout_secs: bt.peer_connect_timeout_secs,
            peer_read_write_timeout_secs: bt.peer_read_write_timeout_secs,
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

/// Traffic-light verdict for BitTorrent incoming port & network reachability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BtPortLight {
    #[default]
    Unknown,
    /// Local listener healthy and public inbound reachability verified (port open, IPv6 reachable, or inbound peers observed).
    Green,
    /// Local listener healthy for outbound peers, but inbound port appears firewalled / behind CGNAT.
    Yellow,
    /// Local listener failed to bind or did not respond to loopback probe.
    Red,
}

/// Real-time snapshot of the shared BitTorrent session, DHT tables, and connection counters.
#[derive(Debug, Clone, Default)]
pub struct BtSessionStatusSnapshot {
    pub session_ready: bool,
    pub listen_addr: Option<String>,
    pub listen_port: u16,
    pub dual_stack: bool,
    pub encryption_mode: BtEncryptionMode,
    pub detected_tun_adapter: Option<String>,
    pub upnp_enabled: bool,
    pub dht_enabled: bool,
    pub dht_nodes_v4: usize,
    pub dht_nodes_v6: usize,
    pub live_peers: u32,
    pub live_tcp: u32,
    pub live_utp: u32,
    pub live_seeders: u32,
    pub connecting_peers: u32,
    pub seen_peers: u32,
    pub incoming_observed: bool,
    pub tcp_v4_ok: u64,
    pub tcp_v6_ok: u64,
    pub utp_v4_ok: u64,
    pub utp_v6_ok: u64,
    pub tcp_v4_err: u64,
    pub tcp_v6_err: u64,
    pub utp_v4_err: u64,
    pub utp_v6_err: u64,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
}

/// Detailed diagnostic report produced by `BitTorrentEngine::check_port_status`.
#[derive(Debug, Clone)]
pub struct BtPortCheckReport {
    pub light: BtPortLight,
    pub checked_at: String,
    pub listen_addr: String,
    pub listen_port: u16,
    pub dual_stack: bool,
    pub encryption_mode: BtEncryptionMode,
    pub detected_tun_adapter: Option<String>,
    pub local_v4_ok: bool,
    pub local_v6_ok: Option<bool>,
    pub public_ipv4: Option<String>,
    pub public_ipv6: Option<String>,
    pub external_port_open: Option<bool>,
    pub incoming_observed: bool,
    pub dht_nodes_v4: usize,
    pub dht_nodes_v6: usize,
    pub summary: String,
    pub advice: String,
}

/// Pure-Rust BitTorrent / Magnet download engine powered by `librqbit`,
/// using a long-lived pre-warmed shared session with persisted DHT routing table,
/// TCP+uTP listeners, and full custom tracker injection for instant task startup.
pub struct BitTorrentEngine {
    data_dir: Option<PathBuf>,
    warm: Mutex<Option<WarmSession>>,
    incoming_observed: Arc<AtomicBool>,
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
            incoming_observed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a `BitTorrentEngine` that persists its DHT routing table under `data_dir`.
    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            data_dir: Some(data_dir),
            warm: Mutex::new(None),
            incoming_observed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Non-blocking snapshot of the currently warm BitTorrent session for live UI telemetry.
    pub async fn live_session_status(&self, bt: &BtConfig) -> BtSessionStatusSnapshot {
        let incoming_observed = self.incoming_observed.load(Ordering::Relaxed);
        let detected_tun_adapter = detect_tun_adapter();
        let guard = self.warm.lock().await;
        let Some(warm) = guard.as_ref() else {
            return BtSessionStatusSnapshot {
                session_ready: false,
                listen_addr: None,
                listen_port: bt.listen_port,
                dual_stack: !bt.ipv4_only,
                encryption_mode: bt.encryption_mode,
                detected_tun_adapter,
                upnp_enabled: bt.enable_upnp,
                dht_enabled: bt.enable_dht,
                incoming_observed,
                ..Default::default()
            };
        };

        let sess = &warm.session;
        let snap = sess.stats_snapshot();
        let listen_addr = sess.listen_addr();
        let listen_port = sess
            .announce_port()
            .or_else(|| listen_addr.map(|a| a.port()))
            .unwrap_or(bt.listen_port);
        let dual_stack = listen_addr.map(|a| a.is_ipv6()).unwrap_or(!bt.ipv4_only);
        let (dht_nodes_v4, dht_nodes_v6) = sess
            .get_dht()
            .map(|d| {
                let ds = d.stats();
                (ds.routing_table_size, ds.routing_table_size_v6)
            })
            .unwrap_or((0, 0));

        BtSessionStatusSnapshot {
            session_ready: true,
            listen_addr: listen_addr.map(|a| a.to_string()),
            listen_port,
            dual_stack,
            encryption_mode: bt.encryption_mode,
            detected_tun_adapter,
            upnp_enabled: bt.enable_upnp,
            dht_enabled: bt.enable_dht,
            dht_nodes_v4,
            dht_nodes_v6,
            live_peers: snap.peers.live,
            live_tcp: snap.peers.live_tcp,
            live_utp: snap.peers.live_utp,
            live_seeders: snap.peers.live_seeders,
            connecting_peers: snap.peers.connecting,
            seen_peers: snap.peers.seen,
            incoming_observed,
            tcp_v4_ok: snap.connections.tcp.v4.successes,
            tcp_v6_ok: snap.connections.tcp.v6.successes,
            utp_v4_ok: snap.connections.utp.v4.successes,
            utp_v6_ok: snap.connections.utp.v6.successes,
            tcp_v4_err: snap.connections.tcp.v4.errors,
            tcp_v6_err: snap.connections.tcp.v6.errors,
            utp_v4_err: snap.connections.utp.v4.errors,
            utp_v6_err: snap.connections.utp.v6.errors,
            download_speed_bps: (snap.download_speed.mbps * 1024.0 * 1024.0).max(0.0) as u64,
            upload_speed_bps: (snap.upload_speed.mbps * 1024.0 * 1024.0).max(0.0) as u64,
        }
    }

    /// Run a real-time local loopback + public IPv4/IPv6 + external TCP port reachability check.
    pub async fn check_port_status(
        &self,
        bt: &BtConfig,
        fallback_dir: &Path,
        proxy_url: Option<&str>,
    ) -> BtPortCheckReport {
        let checked_at = chrono::Local::now().format("%H:%M:%S").to_string();
        let incoming_observed = self.incoming_observed.load(Ordering::Relaxed);
        let detected_tun_adapter = detect_tun_adapter();

        let session = match self
            .get_or_create_session(bt, fallback_dir, proxy_url)
            .await
        {
            Ok((s, _)) => s,
            Err(e) => {
                return BtPortCheckReport {
                    light: BtPortLight::Red,
                    checked_at,
                    listen_addr: "未监听".to_string(),
                    listen_port: bt.listen_port,
                    dual_stack: !bt.ipv4_only,
                    encryption_mode: bt.encryption_mode,
                    detected_tun_adapter,
                    local_v4_ok: false,
                    local_v6_ok: None,
                    public_ipv4: None,
                    public_ipv6: None,
                    external_port_open: None,
                    incoming_observed,
                    dht_nodes_v4: 0,
                    dht_nodes_v6: 0,
                    summary: format!("BT 会话未能启动监听：{e}"),
                    advice: "请检查端口是否被其他程序占用，或在 BT / Tracker 页面更换固定端口（如 16881 / 26881）".to_string(),
                };
            }
        };

        let listen_addr_opt = session.listen_addr();
        let listen_port = session
            .announce_port()
            .or_else(|| listen_addr_opt.map(|a| a.port()))
            .unwrap_or(bt.listen_port);
        let dual_stack = listen_addr_opt
            .map(|a| a.is_ipv6())
            .unwrap_or(!bt.ipv4_only);
        let listen_addr = listen_addr_opt
            .map(|a| a.to_string())
            .unwrap_or_else(|| format!("0.0.0.0:{listen_port}"));
        let (dht_nodes_v4, dht_nodes_v6) = session
            .get_dht()
            .map(|d| {
                let ds = d.stats();
                (ds.routing_table_size, ds.routing_table_size_v6)
            })
            .unwrap_or((0, 0));

        let (local_v4_ok, local_v6_ok) = if listen_port > 0 {
            let v4 = local_loopback_port_probe(SocketAddr::from((
                Ipv4Addr::LOCALHOST,
                listen_port,
            )))
            .await;
            let v6 = if dual_stack {
                Some(
                    local_loopback_port_probe(SocketAddr::from((
                        Ipv6Addr::LOCALHOST,
                        listen_port,
                    )))
                    .await,
                )
            } else {
                None
            };
            (v4, v6)
        } else {
            (false, None)
        };

        let (public_ipv4, public_ipv6, external_port_open) = tokio::join!(
            probe_public_ip(&["https://api4.ipify.org", "https://4.ident.me"], proxy_url),
            probe_public_ip(&["https://api6.ipify.org", "https://6.ident.me"], proxy_url),
            probe_external_tcp_port(listen_port, proxy_url),
        );

        let (light, summary, mut advice) = evaluate_port_light(
            listen_port,
            dual_stack,
            local_v4_ok,
            local_v6_ok,
            public_ipv6.is_some(),
            external_port_open,
            incoming_observed,
            dht_nodes_v6,
        );

        if let Some(tun) = &detected_tun_adapter {
            let tun_note = match bt.encryption_mode {
                BtEncryptionMode::Disabled => format!(
                    "检测到本机虚拟网卡（{tun}），当前协议加密已关闭，明文 BT 握手极易被 TUN/代理节点直接掐断！强烈建议开启「MSE/PE 优先加密」或将 Siphon 加入 TUN 绕过白名单。"
                ),
                BtEncryptionMode::PreferEncrypted | BtEncryptionMode::ForceEncrypted => format!(
                    "检测到本机虚拟网卡（{tun}），已开启 MSE/PE 加密握手穿透明文拦截；如需绿灯入站或提升 DHT/uTP 性能，可将 siphon.exe 加入该 TUN 的直连/进程绕过规则。"
                ),
            };
            advice = format!("{tun_note} | {advice}");
        }

        BtPortCheckReport {
            light,
            checked_at,
            listen_addr,
            listen_port,
            dual_stack,
            encryption_mode: bt.encryption_mode,
            detected_tun_adapter,
            local_v4_ok,
            local_v6_ok,
            public_ipv4,
            public_ipv6,
            external_port_open,
            incoming_observed,
            dht_nodes_v4,
            dht_nodes_v6,
            summary,
            advice,
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
        let opts = build_session_options_with_dht_path(bt, proxy_url, dht_file.clone());

        let session = match Session::new_with_opts(fallback_dir.to_path_buf(), opts).await {
            Ok(s) => s,
            Err(primary_err) => {
                // Fallback 1: if a fixed port was already occupied, keep IPv4+IPv6 dual-stack on ephemeral port 0
                let mut retry_bt = bt.clone();
                let mut created = None;
                if retry_bt.listen_port > 0 {
                    retry_bt.listen_port = 0;
                    let retry_opts = build_session_options_with_dht_path(
                        &retry_bt,
                        proxy_url,
                        dht_file.clone(),
                    );
                    if let Ok(s) =
                        Session::new_with_opts(fallback_dir.to_path_buf(), retry_opts).await
                    {
                        created = Some(s);
                    }
                }
                // Fallback 2: if IPv6 is disabled at the OS/adapter level, gracefully fall back to IPv4
                if created.is_none() && !bt.ipv4_only {
                    let mut v4_bt = bt.clone();
                    v4_bt.ipv4_only = true;
                    let v4_opts = build_session_options_with_dht_path(
                        &v4_bt,
                        proxy_url,
                        dht_file.clone(),
                    );
                    match Session::new_with_opts(fallback_dir.to_path_buf(), v4_opts).await {
                        Ok(s) => created = Some(s),
                        Err(_) if v4_bt.listen_port > 0 => {
                            v4_bt.listen_port = 0;
                            let v4_any_opts = build_session_options_with_dht_path(
                                &v4_bt,
                                proxy_url,
                                dht_file,
                            );
                            if let Ok(s) =
                                Session::new_with_opts(fallback_dir.to_path_buf(), v4_any_opts)
                                    .await
                            {
                                created = Some(s);
                            }
                        }
                        Err(_) => {}
                    }
                }
                match created {
                    Some(s) => s,
                    None => {
                        return Err(SiphonError::Network(format!(
                            "BitTorrent 会话初始化失败: {primary_err}"
                        )));
                    }
                }
            }
        };

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

    let listen_addr: SocketAddr = if bt.ipv4_only {
        (Ipv4Addr::UNSPECIFIED, bt.listen_port).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, bt.listen_port).into()
    };

    let listen = Some(ListenerOptions {
        mode: ListenerMode::TcpAndUtp,
        listen_addr,
        enable_upnp_port_forwarding: bt.enable_upnp,
        ipv4_only: bt.ipv4_only,
        ..Default::default()
    });

    let socks_proxy = proxy_url
        .map(str::trim)
        .filter(|p| p.to_ascii_lowercase().starts_with("socks5://"))
        .map(str::to_string);

    let connect_secs = bt.peer_connect_timeout_secs.clamp(3, 60);
    let rw_secs = bt.peer_read_write_timeout_secs.clamp(15, 180);
    let keep_alive_secs = (rw_secs * 2 / 3).clamp(15, 90);
    let encryption_mode = match bt.encryption_mode {
        BtEncryptionMode::PreferEncrypted => EncryptionMode::PreferEncrypted,
        BtEncryptionMode::ForceEncrypted => EncryptionMode::ForceEncrypted,
        BtEncryptionMode::Disabled => EncryptionMode::Disabled,
    };

    let connect = Some(ConnectionOptions {
        enable_tcp: true,
        proxy_url: socks_proxy,
        peer_opts: Some(PeerConnectionOptions {
            connect_timeout: Some(Duration::from_secs(connect_secs)),
            read_write_timeout: Some(Duration::from_secs(rw_secs)),
            keep_alive_interval: Some(Duration::from_secs(keep_alive_secs)),
            encryption_mode: Some(encryption_mode),
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
        ipv4_only: bt.ipv4_only,
        // Keep librqbit's native client name consistent with its `-rQ...` peer_id to avoid anti-leech mismatch bans
        client_name_and_version: None,
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
        let stack_label = if ctx.bt_config.ipv4_only {
            "仅 IPv4"
        } else {
            "IPv4+IPv6双栈"
        };
        let enc_label = match ctx.bt_config.encryption_mode {
            BtEncryptionMode::PreferEncrypted => "MSE/PE优先加密",
            BtEncryptionMode::ForceEncrypted => "MSE/PE强制加密",
            BtEncryptionMode::Disabled => "明文模式",
        };
        let bound_port = session
            .announce_port()
            .unwrap_or(ctx.bt_config.listen_port);
        push_log(
            &ctx,
            "info",
            format!(
                "BitTorrent 会话{}（{} ms，{}，{}，TCP+uTP 端口 {}，超时 {}s/{}s，DHT={}，注入 Tracker={} 条{}）-> {}",
                if reused { "热复用" } else { "已初始化" },
                t0.elapsed().as_millis(),
                stack_label,
                enc_label,
                bound_port,
                ctx.bt_config.peer_connect_timeout_secs.clamp(3, 60),
                ctx.bt_config.peer_read_write_timeout_secs.clamp(15, 180),
                ctx.bt_config.enable_dht,
                tracker_count,
                selected_note,
                output_dir.display()
            ),
        );

        let bound_addr_str = session
            .listen_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|| format!("0.0.0.0:{bound_port}"));
        let actual_dual_stack = session
            .listen_addr()
            .map(|a| a.is_ipv6())
            .unwrap_or(!ctx.bt_config.ipv4_only);
        let initial_hash = parse_magnet_info_hash(&ctx.task.original_url).unwrap_or_default();
        let (init_dht_v4, init_dht_v6) = session
            .get_dht()
            .map(|d| {
                let ds = d.stats();
                (ds.routing_table_size, ds.routing_table_size_v6)
            })
            .unwrap_or((0, 0));
        let _ = ctx.updates_tx.send(EngineUpdate::BtTelemetry(BtLiveTelemetry {
            info_hash: initial_hash,
            phase: "拉取元数据".to_string(),
            listen_addr: bound_addr_str.clone(),
            listen_port: bound_port,
            dual_stack: actual_dual_stack,
            tracker_count,
            dht_nodes_v4: init_dht_v4,
            dht_nodes_v6: init_dht_v6,
            ..Default::default()
        }));

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

        let resolved_info_hash = handle.info_hash().as_string();
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
        let mut tick_idx = 0u64;

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

            if tick_idx.is_multiple_of(2) || stats.finished {
                let (dht_v4, dht_v6) = session
                    .get_dht()
                    .map(|d| {
                        let ds = d.stats();
                        (ds.routing_table_size, ds.routing_table_size_v6)
                    })
                    .unwrap_or((0, 0));

                let total_pieces = handle
                    .with_metadata(|m| m.info.lengths().total_pieces() as u64)
                    .unwrap_or(0);

                let mut live_ipv6 = 0u32;
                let mut incoming_peers = 0u32;
                let mut top_peers = Vec::new();
                if let Some(live_handle) = handle.live() {
                    let peer_snap = live_handle.per_peer_stats_snapshot(Default::default());
                    for (addr, ps) in peer_snap.peers {
                        let is_ipv6 = addr.starts_with('[');
                        let incoming = ps.counters.incoming_connections > 0;
                        if is_ipv6 {
                            live_ipv6 += 1;
                        }
                        if incoming {
                            incoming_peers += 1;
                            self.incoming_observed.store(true, Ordering::Relaxed);
                        }
                        let transport = ps
                            .conn_kind
                            .map(|k| k.to_string().to_ascii_uppercase())
                            .unwrap_or_else(|| "TCP".to_string());
                        top_peers.push(BtPeerBrief {
                            addr,
                            is_ipv6,
                            transport,
                            incoming,
                            client: ps.client_name.unwrap_or_default(),
                            downloaded_bytes: ps.counters.fetched_bytes,
                            uploaded_bytes: ps.counters.uploaded_bytes,
                        });
                    }
                    top_peers.sort_by(|a, b| {
                        (b.downloaded_bytes + b.uploaded_bytes)
                            .cmp(&(a.downloaded_bytes + a.uploaded_bytes))
                            .then_with(|| b.incoming.cmp(&a.incoming))
                    });
                    top_peers.truncate(8);
                }

                let (
                    live_tcp,
                    live_utp,
                    live_seeders,
                    connecting_peers,
                    queued_peers,
                    dead_peers,
                    upload_speed_bps,
                    uploaded_bytes,
                    checked_pieces,
                ) = if let Some(live) = stats.live.as_ref() {
                    let ps = &live.snapshot.peer_stats;
                    (
                        ps.live_tcp,
                        ps.live_utp,
                        ps.live_seeders,
                        ps.connecting,
                        ps.queued,
                        ps.dead,
                        (live.upload_speed.mbps * 1024.0 * 1024.0).max(0.0) as u64,
                        stats.uploaded_bytes.max(live.snapshot.uploaded_bytes),
                        live.snapshot.downloaded_and_checked_pieces,
                    )
                } else {
                    (0, 0, 0, 0, 0, 0, 0, stats.uploaded_bytes, 0)
                };

                let phase = if stats.finished {
                    "已完成"
                } else if stats.total_bytes == 0 {
                    "拉取元数据"
                } else if live_peers > 0 && speed_bps > 0 {
                    "Swarm 传输中"
                } else if live_peers > 0 {
                    "已连节点等待数据"
                } else if connecting_peers > 0 || seen_peers > 0 {
                    "正在连接 Peer"
                } else {
                    "正在搜寻节点"
                };

                let _ = ctx.updates_tx.send(EngineUpdate::BtTelemetry(BtLiveTelemetry {
                    info_hash: resolved_info_hash.clone(),
                    phase: phase.to_string(),
                    listen_addr: bound_addr_str.clone(),
                    listen_port: bound_port,
                    dual_stack: actual_dual_stack,
                    live_peers,
                    live_tcp,
                    live_utp,
                    live_seeders,
                    live_ipv6,
                    incoming_peers,
                    connecting_peers,
                    queued_peers,
                    seen_peers,
                    dead_peers,
                    upload_speed_bps,
                    uploaded_bytes,
                    checked_pieces,
                    total_pieces,
                    tracker_count,
                    dht_nodes_v4: dht_v4,
                    dht_nodes_v6: dht_v6,
                    top_peers,
                }));
            }
            tick_idx = tick_idx.wrapping_add(1);

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

pub fn parse_magnet_info_hash(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw.trim()).ok()?;
    for (k, v) in parsed.query_pairs() {
        if k == "xt"
            && let Some(hash) = v.rsplit(':').next()
            && !hash.trim().is_empty()
        {
            return Some(hash.trim().to_ascii_lowercase());
        }
    }
    None
}

/// Detect active TUN / virtual proxy network adapters (e.g. `singbox_tun`, Clash, Mihomo, Wintun)
/// that often sniff & intercept plaintext BitTorrent traffic or block incoming P2P ports.
pub fn detect_tun_adapter() -> Option<String> {
    let ifaces = NetworkInterface::show().ok()?;
    for iface in ifaces {
        if iface.addr.is_empty() {
            continue;
        }
        let low = iface.name.to_ascii_lowercase();
        let is_tun = low.contains("singbox")
            || low.contains("sing-box")
            || low.contains("sing-tun")
            || low.contains("clash")
            || low.contains("mihomo")
            || low.contains("wintun")
            || low.contains("flclash")
            || low.contains("nekobox")
            || low.contains("nekoray")
            || low.contains("v2ray")
            || low.contains("metacubex")
            || low.starts_with("tun")
            || low.starts_with("utun")
            || low.ends_with("_tun");
        if is_tun {
            return Some(iface.name);
        }
    }
    None
}

async fn local_loopback_port_probe(addr: SocketAddr) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_millis(900), TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

async fn probe_public_ip(endpoints: &[&str], proxy_url: Option<&str>) -> Option<String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .user_agent(format!("Siphon/{}", env!("CARGO_PKG_VERSION")));
    if let Some(proxy) = proxy_url
        && !proxy.trim().is_empty()
        && let Ok(p) = reqwest::Proxy::all(proxy.trim())
    {
        builder = builder.proxy(p);
    }
    let client = builder.build().ok()?;
    for url in endpoints {
        if let Ok(resp) = client.get(*url).send().await
            && resp.status().is_success()
            && let Ok(txt) = resp.text().await
        {
            let trimmed = txt.trim();
            if !trimmed.is_empty() && trimmed.len() <= 64 && !trimmed.contains('<') {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

async fn probe_external_tcp_port(port: u16, proxy_url: Option<&str>) -> Option<bool> {
    if port == 0 {
        return None;
    }
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .user_agent(format!("Siphon/{}", env!("CARGO_PKG_VERSION")));
    if let Some(proxy) = proxy_url
        && !proxy.trim().is_empty()
        && let Ok(p) = reqwest::Proxy::all(proxy.trim())
    {
        builder = builder.proxy(p);
    }
    let client = builder.build().ok()?;

    let primary_url = format!("https://portcheck.transmissionbt.com/{port}");
    if let Ok(resp) = client.get(&primary_url).send().await
        && resp.status().is_success()
        && let Ok(txt) = resp.text().await
    {
        match txt.trim() {
            "1" => return Some(true),
            "0" => return Some(false),
            _ => {}
        }
    }

    let fallback_url = format!("https://ifconfig.co/port/{port}");
    if let Ok(resp) = client
        .get(&fallback_url)
        .header("Accept", "application/json")
        .send()
        .await
        && resp.status().is_success()
        && let Ok(txt) = resp.text().await
    {
        if txt.contains("\"reachable\":true") || txt.contains("\"reachable\": true") {
            return Some(true);
        }
        if txt.contains("\"reachable\":false") || txt.contains("\"reachable\": false") {
            return Some(false);
        }
    }

    None
}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_port_light(
    listen_port: u16,
    dual_stack: bool,
    local_v4_ok: bool,
    local_v6_ok: Option<bool>,
    has_public_ipv6: bool,
    external_port_open: Option<bool>,
    incoming_observed: bool,
    dht_nodes_v6: usize,
) -> (BtPortLight, String, String) {
    let local_ok = local_v4_ok || local_v6_ok == Some(true);
    if listen_port == 0 || !local_ok {
        return (
            BtPortLight::Red,
            format!("红灯 · 本地端口 :{listen_port} 未响应监听"),
            "本地 BT 监听套接字未就绪，请检查端口占用或 Windows 防火墙拦截后重试".to_string(),
        );
    }

    if external_port_open == Some(true) || incoming_observed {
        let via = if external_port_open == Some(true) && incoming_observed {
            "外网端口探测开放，且已实测收到外部 Peer 主动连入"
        } else if external_port_open == Some(true) {
            "外网 TCP 端口探测开放，外部 Peer 可直接连入"
        } else {
            "本会话已实测收到外部 Peer 主动入站连接"
        };
        return (
            BtPortLight::Green,
            format!("绿灯 · 端口 :{listen_port} 入站畅通"),
            format!("{via}，当前处于最佳 P2P 互联状态"),
        );
    }

    if dual_stack && local_v6_ok == Some(true) && (has_public_ipv6 || dht_nodes_v6 > 0) {
        return (
            BtPortLight::Green,
            format!("绿灯 · IPv6 双栈可达 (:{listen_port})"),
            if external_port_open == Some(false) {
                "IPv4 处于 NAT/防火墙后，但检测到公网 IPv6 双栈可用，IPv6 Peer 可直连互通（如需 IPv4 也绿灯，请在路由器转发此端口）".to_string()
            } else {
                "检测到公网 IPv6 双栈就绪，支持直连 IPv6 公网 Peer 与双栈 DHT".to_string()
            },
        );
    }

    let summary = if external_port_open == Some(false) {
        format!("黄灯 · 端口 :{listen_port} 外网入站阻塞（仅出站）")
    } else {
        format!("黄灯 · 本地 :{listen_port} 监听正常，外网入站未确认")
    };
    let advice = format!(
        "当前仅能主动外连公网 Peer（类似黄灯阻塞）。改善建议：1) 检查路由器是否开启 IPv6；2) 在路由器为 TCP/UDP 端口 {listen_port} 配置端口转发或开启 UPnP；3) 在 Windows 防火墙放行 Siphon 入站"
    );
    (BtPortLight::Yellow, summary, advice)
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
        // Default config enables IPv4+IPv6 dual-stack on fixed port 16881 with relaxed timeouts & native rqbit identity
        let default_bt = BtConfig::default();
        let default_sess = build_session_options(&default_bt, None);
        assert!(!default_sess.ipv4_only);
        assert!(default_sess.client_name_and_version.is_none());
        let def_listen = default_sess.listen.expect("default listen present");
        assert!(def_listen.listen_addr.is_ipv6());
        assert_eq!(def_listen.listen_addr.port(), 16881);
        assert!(!def_listen.ipv4_only);
        let def_peer_opts = default_sess
            .connect
            .as_ref()
            .and_then(|c| c.peer_opts)
            .expect("peer opts present");
        assert_eq!(def_peer_opts.connect_timeout, Some(Duration::from_secs(12)));
        assert_eq!(
            def_peer_opts.read_write_timeout,
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            def_peer_opts.keep_alive_interval,
            Some(Duration::from_secs(40))
        );

        let bt = BtConfig {
            enable_dht: false,
            enable_lsd: false,
            enable_upnp: true,
            disable_trackers: false,
            ipv4_only: true,
            listen_port: 6889,
            peer_connect_timeout_secs: 15,
            peer_read_write_timeout_secs: 90,
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
        assert!(listen.listen_addr.is_ipv4());
        assert_eq!(listen.listen_addr.port(), 6889);
        assert!(listen.enable_upnp_port_forwarding);
        assert_eq!(
            sess.connect.as_ref().and_then(|c| c.proxy_url.as_deref()),
            Some("socks5://127.0.0.1:1080")
        );
        let peer_opts = sess.connect.as_ref().and_then(|c| c.peer_opts).unwrap();
        assert_eq!(peer_opts.connect_timeout, Some(Duration::from_secs(15)));
        assert_eq!(peer_opts.read_write_timeout, Some(Duration::from_secs(90)));
        assert_eq!(peer_opts.keep_alive_interval, Some(Duration::from_secs(60)));

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

    #[test]
    fn port_light_verdict_classifies_green_yellow_and_red_accurately() {
        // Red: local loopback failed
        let (l_red, _, _) = evaluate_port_light(16881, true, false, Some(false), false, None, false, 0);
        assert_eq!(l_red, BtPortLight::Red);

        // Green: external port open
        let (l_green_ext, _, _) =
            evaluate_port_light(16881, true, true, Some(true), false, Some(true), false, 0);
        assert_eq!(l_green_ext, BtPortLight::Green);

        // Green: public IPv6 dual-stack reachable even if IPv4 NATed
        let (l_green_v6, _, _) =
            evaluate_port_light(16881, true, true, Some(true), true, Some(false), false, 12);
        assert_eq!(l_green_v6, BtPortLight::Green);

        // Yellow: local OK, but external closed & no public IPv6 / inbound
        let (l_yellow, _, advice) =
            evaluate_port_light(16881, true, true, Some(true), false, Some(false), false, 0);
        assert_eq!(l_yellow, BtPortLight::Yellow);
        assert!(advice.contains("16881"));
    }

    #[tokio::test]
    async fn live_session_status_and_local_loopback_probe_work() {
        let dir = tempfile::tempdir().unwrap();
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

        let before = engine.live_session_status(&bt).await;
        assert!(!before.session_ready);

        let (_s, _) = engine
            .get_or_create_session(&bt, dir.path(), None)
            .await
            .unwrap();

        let after = engine.live_session_status(&bt).await;
        assert!(after.session_ready);
        assert!(after.listen_port > 0);
        assert!(local_loopback_port_probe(SocketAddr::from((Ipv4Addr::LOCALHOST, after.listen_port))).await);
    }

    #[tokio::test]
    async fn encrypted_mse_sessions_transfer_torrent_over_loopback() {
        let seed_dir = tempfile::tempdir().unwrap();
        let leech_dir = tempfile::tempdir().unwrap();

        // Create a 128 KiB deterministic payload and package it into a real `.torrent`
        let payload: Vec<u8> = (0..131_072usize)
            .map(|i| ((i * 73 + 19) % 251) as u8)
            .collect();
        let src_file = seed_dir.path().join("encrypted_sample.bin");
        tokio::fs::write(&src_file, &payload).await.unwrap();

        let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
        let created = librqbit::create_torrent(
            &src_file,
            librqbit::CreateTorrentOptions {
                piece_length: Some(32 * 1024),
                ..Default::default()
            },
            &spawner,
        )
        .await
        .unwrap();
        let torrent_bytes = created.as_bytes().unwrap();

        let bt = BtConfig {
            enable_dht: false,
            enable_lsd: false,
            enable_upnp: false,
            disable_trackers: true,
            ipv4_only: true,
            encryption_mode: BtEncryptionMode::ForceEncrypted,
            listen_port: 0,
            auto_append_trackers: false,
            ..Default::default()
        };

        let seeder = Session::new_with_opts(
            seed_dir.path().to_path_buf(),
            build_session_options(&bt, None),
        )
        .await
        .unwrap();
        let seeder_port = seeder.listen_addr().unwrap().port();
        let seeder_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, seeder_port));

        let _seed_resp = seeder
            .add_torrent(
                AddTorrent::from_bytes(torrent_bytes.clone()),
                Some(AddTorrentOptions {
                    overwrite: true,
                    disable_trackers: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap();

        let leecher = Session::new_with_opts(
            leech_dir.path().to_path_buf(),
            build_session_options(&bt, None),
        )
        .await
        .unwrap();

        let leech_resp = leecher
            .add_torrent(
                AddTorrent::from_bytes(torrent_bytes),
                Some(AddTorrentOptions {
                    overwrite: true,
                    disable_trackers: true,
                    initial_peers: Some(vec![seeder_addr]),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        let leech_handle = leech_resp.into_handle().unwrap();

        tokio::time::timeout(Duration::from_secs(15), leech_handle.wait_until_completed())
            .await
            .expect("MSE/PE ForceEncrypted loopback transfer timed out")
            .unwrap();

        let downloaded = tokio::fs::read(leech_dir.path().join("encrypted_sample.bin"))
            .await
            .unwrap();
        assert_eq!(downloaded, payload);

        leecher.stop().await;
        seeder.stop().await;
    }
}
