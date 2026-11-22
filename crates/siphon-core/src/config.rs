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
