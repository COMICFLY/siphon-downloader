use crate::error::{Result, SiphonError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ListDensity {
    #[default]
    Comfortable,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    ZhCn,
    EnUs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QueueCompleteAction {
    #[default]
    None,
    ExitApp,
    Sleep,
    Hibernate,
    Shutdown,
    RunScript,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleConfig {
    /// Automatically resume paused/queued tasks at `start_time` (`HH:MM`, 24-hour local time).
    pub start_enabled: bool,
    pub start_time: String,
    /// Automatically pause active tasks at `stop_time` (`HH:MM`, 24-hour local time).
    pub stop_enabled: bool,
    pub stop_time: String,
    /// Automatically cap global download speed during `[speed_start_time, speed_end_time]`.
    pub speed_schedule_enabled: bool,
    pub speed_start_time: String,
    pub speed_end_time: String,
    /// Scheduled global speed limit in KB/s (e.g. `5120` = 5 MB/s).
    pub scheduled_speed_limit_kbps: u64,
    /// Action executed when all active/queued downloads finish.
    pub on_queue_complete: QueueCompleteAction,
    /// Custom command/script executed when `on_queue_complete == RunScript`.
    pub post_queue_script: String,
    /// Reset `on_queue_complete` back to `None` after triggering once to prevent accidental repeated shutdowns.
    pub one_shot_complete_action: bool,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            start_enabled: false,
            start_time: "02:00".to_string(),
            stop_enabled: false,
            stop_time: "07:00".to_string(),
            speed_schedule_enabled: false,
            speed_start_time: "09:00".to_string(),
            speed_end_time: "23:00".to_string(),
            scheduled_speed_limit_kbps: 5120,
            on_queue_complete: QueueCompleteAction::None,
            post_queue_script: String::new(),
            one_shot_complete_action: true,
        }
    }
}

/// Parse `HH:MM` (24-hour) into minutes since midnight (`0..1440`).
pub fn parse_hhmm(raw: &str) -> Option<u16> {
    let (h, m) = raw.trim().split_once(':')?;
    let hh: u16 = h.trim().parse().ok()?;
    let mm: u16 = m.trim().parse().ok()?;
    if hh < 24 && mm < 60 {
        Some(hh * 60 + mm)
    } else {
        None
    }
}

/// Check whether `now_mins` falls within `[start_hhmm, end_hhmm)` (supports overnight wrap like `23:00` -> `07:00`).
pub fn is_time_in_window(now_mins: u16, start_hhmm: &str, end_hhmm: &str) -> bool {
    let (Some(start), Some(end)) = (parse_hhmm(start_hhmm), parse_hhmm(end_hhmm)) else {
        return false;
    };
    if start == end {
        return false;
    }
    if start < end {
        now_mins >= start && now_mins < end
    } else {
        now_mins >= start || now_mins < end
    }
}

/// Default curated public BitTorrent tracker subscription lists.
pub fn default_tracker_subscribe_urls() -> Vec<String> {
    vec![
        "https://cf.trackerslist.com/best.txt".to_string(),
        "https://raw.githubusercontent.com/ngosang/trackerslist/master/trackers_best.txt"
            .to_string(),
    ]
}

/// Parse, validate, and deduplicate BitTorrent tracker URLs from multi-line / comma / whitespace text.
pub fn parse_tracker_list(raw: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        for tok in line.split([',', ';', ' ', '\t']) {
            let candidate = tok.trim().trim_matches(|c| matches!(c, '"' | '\''));
            if candidate.is_empty() {
                continue;
            }
            let low = candidate.to_ascii_lowercase();
            let valid_scheme = low.starts_with("udp://")
                || low.starts_with("http://")
                || low.starts_with("https://")
                || low.starts_with("ws://")
                || low.starts_with("wss://");
            if valid_scheme && url::Url::parse(candidate).is_ok() && seen.insert(candidate.to_string()) {
                out.push(candidate.to_string());
            }
        }
    }
    out
}

/// BitTorrent MSE/PE protocol stream encryption mode.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BtEncryptionMode {
    /// Attempt MSE/PE (768-bit DH + RC4) encrypted handshake first; fall back to plaintext if peer rejects MSE.
    #[default]
    PreferEncrypted,
    /// Require MSE/PE RC4 encryption for all outgoing and incoming BitTorrent connections.
    ForceEncrypted,
    /// Plaintext BitTorrent handshakes only (not recommended behind TUN proxies or strict ISPs).
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BtConfig {
    /// Enable BitTorrent DHT network for trackerless peer discovery.
    pub enable_dht: bool,
    /// Enable Local Service Discovery (LSD) on LAN.
    pub enable_lsd: bool,
    /// Enable UPnP automatic port forwarding on the router.
    pub enable_upnp: bool,
    /// Disable tracker announcements completely (DHT-only mode).
    pub disable_trackers: bool,
    /// Restrict BitTorrent to IPv4 only (`false` = IPv4+IPv6 dual-stack, strongly recommended).
    #[serde(default)]
    pub ipv4_only: bool,
    /// BitTorrent MSE/PE stream encryption mode (default: `PreferEncrypted`).
    #[serde(default)]
    pub encryption_mode: BtEncryptionMode,
    /// Incoming TCP/uTP listen port (fixed default `16881`; `0` = automatic random port).
    pub listen_port: u16,
    /// Peer TCP/uTP connection timeout in seconds.
    pub peer_connect_timeout_secs: u64,
    /// Peer socket read/write idle timeout in seconds (higher values prevent dropping choked peers).
    #[serde(default = "default_bt_rw_timeout_secs")]
    pub peer_read_write_timeout_secs: u64,
    /// Maximum concurrent peers per torrent task.
    pub max_peers_per_torrent: usize,
    /// Automatically append custom & subscribed trackers to every magnet / `.torrent` task.
    pub auto_append_trackers: bool,
    /// User-defined custom tracker URLs (`udp://`, `http://`, `https://`, `ws://`, `wss://`).
    pub custom_trackers: Vec<String>,
    /// Online tracker list subscription URLs for one-click sync.
    pub tracker_subscribe_urls: Vec<String>,
    /// Trackers fetched from `tracker_subscribe_urls`.
    pub synced_trackers: Vec<String>,
    /// Human-readable timestamp of the last successful tracker sync.
    pub last_tracker_sync_at: Option<String>,
    /// Automatically refresh subscribed trackers when Siphon starts.
    pub auto_sync_trackers_on_start: bool,
}

fn default_bt_rw_timeout_secs() -> u64 {
    60
}

impl Default for BtConfig {
    fn default() -> Self {
        Self {
            enable_dht: true,
            enable_lsd: true,
            enable_upnp: true,
            disable_trackers: false,
            ipv4_only: false,
            encryption_mode: BtEncryptionMode::PreferEncrypted,
            listen_port: 16881,
            peer_connect_timeout_secs: 12,
            peer_read_write_timeout_secs: default_bt_rw_timeout_secs(),
            max_peers_per_torrent: 160,
            auto_append_trackers: true,
            custom_trackers: vec![
                "udp://tracker.opentrackr.org:1337/announce".to_string(),
                "https://tracker.gbitt.info:443/announce".to_string(),
                "http://tracker.gbitt.info:80/announce".to_string(),
                "udp://open.demonii.com:1337/announce".to_string(),
                "udp://open.stealth.si:80/announce".to_string(),
                "udp://tracker.torrent.eu.org:451/announce".to_string(),
                "udp://exodus.desync.com:6969/announce".to_string(),
                "udp://explodie.org:6969/announce".to_string(),
                "udp://tracker.openbittorrent.com:6969/announce".to_string(),
                "udp://opentracker.io:6969/announce".to_string(),
                "udp://tracker.tryhackx.org:6969/announce".to_string(),
                "udp://tracker.dler.org:6969/announce".to_string(),
                "http://tracker.mywaifu.best:6969/announce".to_string(),
                "https://tracker.tamersunion.org:443/announce".to_string(),
                "https://tr.burnabyhighstar.com:443/announce".to_string(),
                "udp://tracker-udp.gbitt.info:80/announce".to_string(),
            ],
            tracker_subscribe_urls: default_tracker_subscribe_urls(),
            synced_trackers: Vec::new(),
            last_tracker_sync_at: None,
            auto_sync_trackers_on_start: false,
        }
    }
}

impl BtConfig {
    /// Merge `custom_trackers` and `synced_trackers`, preserving priority and deduplicating valid URLs.
    pub fn effective_trackers(&self) -> Vec<String> {
        let mut combined = String::new();
        for t in &self.custom_trackers {
            combined.push_str(t);
            combined.push('\n');
        }
        for t in &self.synced_trackers {
            combined.push_str(t);
            combined.push('\n');
        }
        parse_tracker_list(&combined)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ed2kServerEntry {
    pub name: String,
    pub host: String,
    pub port: u16,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Curated public eD2k servers for initial configuration.
pub fn default_ed2k_servers() -> Vec<Ed2kServerEntry> {
    vec![
        Ed2kServerEntry {
            name: "eMule Security".to_string(),
            host: "45.82.80.155".to_string(),
            port: 5687,
            enabled: true,
        },
        Ed2kServerEntry {
            name: "eDonkey Server No2".to_string(),
            host: "5.45.85.226".to_string(),
            port: 6584,
            enabled: true,
        },
        Ed2kServerEntry {
            name: "GrupoTS Server".to_string(),
            host: "145.239.2.134".to_string(),
            port: 4661,
            enabled: true,
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Ed2kConfig {
    /// Enable the eD2k protocol handler and link capture.
    pub enabled: bool,
    /// Client nickname announced to eD2k servers and peers.
    pub nickname: String,
    /// eD2k TCP listen port (classic default `4662`).
    pub tcp_port: u16,
    /// eD2k Kad UDP port (classic default `4672`).
    pub udp_port: u16,
    /// Enable Kademlia (Kad) serverless peer network.
    pub enable_kad: bool,
    /// Enable eD2k protocol obfuscation to avoid ISP throttling.
    pub enable_obfuscation: bool,
    /// Maximum number of sources tracked per eD2k file.
    pub max_sources_per_file: u32,
    /// Upload speed limit in KB/s (`0` = unlimited).
    pub upload_limit_kbps: u64,
    /// Verify final downloaded file against the eD2k MD4 root hash.
    pub verify_md4_hash: bool,
    /// Remote `server.met` subscription URL for one-click server list update.
    pub server_met_url: String,
    /// Configured eD2k servers (`host:port`).
    pub servers: Vec<Ed2kServerEntry>,
}

impl Default for Ed2kConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            nickname: "Siphon".to_string(),
            tcp_port: 4662,
            udp_port: 4672,
            enable_kad: true,
            enable_obfuscation: true,
            max_sources_per_file: 400,
            upload_limit_kbps: 0,
            verify_md4_hash: true,
            server_met_url: "http://upd.emule-security.org/server.met".to_string(),
            servers: default_ed2k_servers(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub default_save_dir: PathBuf,
    pub default_connections: u32,
    pub min_split_size_bytes: u64,
    pub max_concurrent_tasks: usize,
    /// Global speed cap in bytes/sec (`0` = unlimited).
    pub global_speed_limit_bps: u64,
    pub theme_mode: ThemeMode,
    /// Primary accent color in `#RRGGBB` format (default `#38BDF8` Cool-Cyan).
    pub accent_color: String,
    /// Download telemetry accent color for the standalone download detail popup (default `#22C55E` Emerald Green).
    pub download_popup_accent: String,
    pub use_mica: bool,
    pub list_density: ListDensity,
    pub language: Language,
    pub clipboard_monitor: bool,
    pub minimize_to_tray: bool,
    pub autostart: bool,
    /// Show a confirmation modal when clicking delete to choose whether to delete local files.
    pub confirm_on_delete: bool,
    /// Default choice for "also delete local files" (pre-selected in dialog, or applied directly when dialog is disabled).
    pub delete_files_by_default: bool,
    /// Enable local HTTP bridge (`127.0.0.1:16800`) and accept browser extension download captures.
    pub browser_integration: bool,
    /// Local loopback port for the browser extension HTTP bridge.
    pub browser_bridge_port: u16,
    /// Pop up the standalone floating capture window when a browser download is intercepted.
    pub browser_capture_popup: bool,
    /// Pop up the standalone floating capture window immediately when a download link is copied to clipboard.
    pub clipboard_capture_popup: bool,
    pub proxy_url: Option<String>,
    pub user_agent: String,
    pub schedule: ScheduleConfig,
    pub bt: BtConfig,
    pub ed2k: Ed2kConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        let default_save_dir = dirs::download_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
            .unwrap_or_else(|| PathBuf::from("."));

        Self {
            default_save_dir,
            default_connections: 8,
            min_split_size_bytes: 1024 * 1024, // 1 MiB
            max_concurrent_tasks: 3,
            global_speed_limit_bps: 0,
            theme_mode: ThemeMode::Dark,
            accent_color: "#38BDF8".to_string(),
            download_popup_accent: "#22C55E".to_string(),
            use_mica: true,
            list_density: ListDensity::Comfortable,
            language: Language::ZhCn,
            clipboard_monitor: true,
            minimize_to_tray: true,
            autostart: false,
            confirm_on_delete: true,
            delete_files_by_default: false,
            browser_integration: true,
            browser_bridge_port: 16800,
            browser_capture_popup: true,
            clipboard_capture_popup: false,
            proxy_url: None,
            user_agent: format!("Siphon/{} (Windows NT 10.0; Win64; x64)", env!("CARGO_PKG_VERSION")),
            schedule: ScheduleConfig::default(),
            bt: BtConfig::default(),
            ed2k: Ed2kConfig::default(),
        }
    }
}

impl AppConfig {
    /// Set an explicit data directory override (e.g. from `--data-dir <PATH>` CLI flag).
    pub fn set_data_dir_override(path: PathBuf) {
        let _ = DATA_DIR_OVERRIDE.set(path);
    }

    /// Default system AppData location (`%APPDATA%\Siphon`).
    pub fn default_app_data_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Siphon")
    }

    fn read_pointer_file(pointer: &Path, relative_base: &Path) -> Option<PathBuf> {
        let raw = std::fs::read_to_string(pointer).ok()?;
        let trimmed = raw.trim().trim_matches('"');
        if trimmed.is_empty() {
            return None;
        }
        let p = PathBuf::from(trimmed);
        let resolved = if p.is_absolute() {
            p
        } else {
            relative_base.join(p)
        };
        Some(resolved.components().collect())
    }

    /// Resolve active Siphon data directory with full user control:
    /// 1. CLI `--data-dir <PATH>`
    /// 2. Environment variable `SIPHON_DATA_DIR`
    /// 3. Portable / Installer pointer `data_dir.txt` beside `siphon.exe`
    /// 4. User pointer `%APPDATA%\Siphon\data_dir.txt` (set via Settings UI)
    /// 5. Default `%APPDATA%\Siphon`
    pub fn app_data_dir() -> PathBuf {
        if let Some(p) = DATA_DIR_OVERRIDE.get() {
            return p.clone();
        }
        if let Ok(env_dir) = std::env::var("SIPHON_DATA_DIR") {
            let trimmed = env_dir.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed).components().collect();
            }
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(exe_dir) = exe.parent()
        {
            let portable_ptr = exe_dir.join("data_dir.txt");
            if let Some(p) = Self::read_pointer_file(&portable_ptr, exe_dir) {
                return p;
            }
        }
        let default_dir = Self::default_app_data_dir();
        let user_ptr = default_dir.join("data_dir.txt");
        if let Some(p) = Self::read_pointer_file(&user_ptr, &default_dir) {
            return p;
        }
        default_dir
    }

    /// Migrate user data (`config.toml`, `siphon.db`, `plugins`, `browser-extension`) into `new_dir`
    /// and persist the pointer so future launches use `new_dir`.
    pub fn migrate_data_dir(old_dir: &Path, new_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(new_dir)?;
        if old_dir != new_dir {
            for fname in ["config.toml", "siphon.db", "siphon.db-wal", "siphon.db-shm"] {
                let src = old_dir.join(fname);
                let dst = new_dir.join(fname);
                if src.is_file() && !dst.exists() {
                    let _ = std::fs::copy(&src, &dst);
                }
            }
            for sub in ["plugins", "browser-extension"] {
                let src = old_dir.join(sub);
                let dst = new_dir.join(sub);
                if src.is_dir() && !dst.exists() {
                    let _ = copy_dir_simple(&src, &dst);
                }
            }
        }

        // Write pointer into portable `data_dir.txt` beside exe if writable & already exists,
        // otherwise into `%APPDATA%\Siphon\data_dir.txt`.
        let mut wrote_portable = false;
        if let Ok(exe) = std::env::current_exe()
            && let Some(exe_dir) = exe.parent()
        {
            let portable_ptr = exe_dir.join("data_dir.txt");
            if portable_ptr.is_file()
                && std::fs::write(&portable_ptr, new_dir.to_string_lossy().as_bytes()).is_ok()
            {
                wrote_portable = true;
            }
        }
        if !wrote_portable {
            let default_dir = Self::default_app_data_dir();
            std::fs::create_dir_all(&default_dir)?;
            let user_ptr = default_dir.join("data_dir.txt");
            if new_dir == default_dir {
                let _ = std::fs::remove_file(&user_ptr);
            } else {
                std::fs::write(&user_ptr, new_dir.to_string_lossy().as_bytes())?;
            }
        }
        Ok(())
    }

    pub fn config_path(data_dir: &Path) -> PathBuf {
        data_dir.join("config.toml")
    }

    pub fn db_path(data_dir: &Path) -> PathBuf {
        data_dir.join("siphon.db")
    }

    pub fn plugins_dir(data_dir: &Path) -> PathBuf {
        data_dir.join("plugins")
    }

    pub fn load_or_default(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        std::fs::create_dir_all(Self::plugins_dir(data_dir))?;
        let path = Self::config_path(data_dir);
        if !path.is_file() {
            let cfg = Self::default();
            cfg.save(data_dir)?;
            return Ok(cfg);
        }
        let raw = std::fs::read_to_string(&path)?;
        let cfg: Self = toml::from_str(&raw).map_err(|e| SiphonError::Config(e.to_string()))?;
        Ok(cfg)
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = Self::config_path(data_dir);
        let content =
            toml::to_string_pretty(self).map_err(|e| SiphonError::Config(e.to_string()))?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

fn copy_dir_simple(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_simple(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_parser_filters_invalid_and_deduplicates() {
        let raw = r#"
            # Comment line
            udp://tracker.opentrackr.org:1337/announce
            http://example.com:6969/announce, https://tracker.example.org/announce
            udp://tracker.opentrackr.org:1337/announce
            ftp://not-a-bt-tracker.com/announce
            not a url at all
            wss://tracker.openwebtorrent.com
        "#;
        let parsed = parse_tracker_list(raw);
        assert_eq!(
            parsed,
            vec![
                "udp://tracker.opentrackr.org:1337/announce",
                "http://example.com:6969/announce",
                "https://tracker.example.org/announce",
                "wss://tracker.openwebtorrent.com",
            ]
        );
    }

    #[test]
    fn bt_and_ed2k_configs_roundtrip_via_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = AppConfig::default();
        cfg.bt.listen_port = 51413;
        cfg.bt.custom_trackers = vec!["udp://custom.tracker:80/announce".into()];
        cfg.bt.synced_trackers = vec![
            "udp://custom.tracker:80/announce".into(),
            "https://synced.tracker/announce".into(),
        ];
        cfg.ed2k.nickname = "SiphonTest".into();
        cfg.ed2k.tcp_port = 14662;
        cfg.ed2k.udp_port = 14672;
        cfg.ed2k.servers.push(Ed2kServerEntry {
            name: "Custom Node".into(),
            host: "127.0.0.1".into(),
            port: 4661,
            enabled: true,
        });

        cfg.save(tmp.path()).unwrap();
        let loaded = AppConfig::load_or_default(tmp.path()).unwrap();
        assert_eq!(loaded.bt.listen_port, 51413);
        assert_eq!(
            loaded.bt.effective_trackers(),
            vec![
                "udp://custom.tracker:80/announce",
                "https://synced.tracker/announce",
            ]
        );
        assert_eq!(loaded.ed2k.nickname, "SiphonTest");
        assert_eq!(loaded.ed2k.tcp_port, 14662);
        assert_eq!(loaded.ed2k.udp_port, 14672);
        assert!(loaded.ed2k.servers.iter().any(|s| s.name == "Custom Node"));
    }
}
