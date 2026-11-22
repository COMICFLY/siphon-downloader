use crate::i18n::Strings;
use crate::icons::{IconKind, icon};
use crate::state::{
    CaptureAction, CapturePopupState, ContextMenuState, DeleteConfirmState, DownloadPopupState,
    ExtInstallModalState, InputTarget, Message, NavSection, NewTaskModalState, RuleDraft,
    ServerTestStatus, SniffEntry, SniffFilterKind, SniffPopupState, UpdateCheckStatus,
};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, SiphonPalette, hairline_rule_style, modal_card_style,
    normalize_hex_color, panel_style, subtle_btn_style,
};
use crate::views;
use iced::futures::SinkExt;
use iced::keyboard::{self, Key, Modifiers};
use iced::mouse;
use iced::widget::{
    button, column, container, image, mouse_area, operation, row, rule, space, stack, text,
};
use iced::{Alignment, Element, Event, Length, Point, Size, Subscription, Task, Theme, stream, window};
use siphon_core::{
    AppConfig, AutomationRule, ChecksumSpec, DownloadManager, DownloadTask, Ed2kServerEntry,
    ExternalPlugin, FileCategory, HashAlgorithm, NewTaskRequest, QueueCompleteAction, RuleAction,
    RuleCondition, RuleEngine, RuleEvalInput, TaskEvent, TaskId, TaskSource, TaskStatus, ThemeMode,
    default_ed2k_servers, default_tracker_subscribe_urls, expand_batch_input, format_speed,
    infer_filename_from_raw_url, is_time_in_window, parse_hhmm, parse_tracker_list,
    sanitize_filename,
};
use siphon_engine_bt::{BitTorrentEngine, sync_trackers_from_urls};
use siphon_engine_ed2k::{
    Ed2kEngine, parse_ed2k_server_link, sync_servers_from_met, test_ed2k_server,
};
use siphon_engine_hls::HlsEngine;
use siphon_engine_http::{HttpEngine, check_for_updates, probe_urls_sizes, sniff_webpage_url};
use siphon_platform_win::{
    BrowserCapturePayload, BrowserSniffPayload, DropEvent, FileIconKey, IpcCommand, TrayAction,
    TrayController, WinHookEvent, apply_win11_backdrop, deploy_browser_extension,
    detect_installed_browsers, execute_power_action, export_browser_extension_zip,
    extract_file_icons_async, extract_urls_from_text, install_url_drop_target,
    install_window_hooks, is_autostart_enabled, is_system_dark_mode,
    launch_browser_extensions_page, launch_silent_installer_update, open_file, open_folder,
    open_url, read_clipboard_text, register_native_messaging_host, register_siphon_protocol,
    reveal_in_explorer, set_autostart, show_window_native, spawn_browser_http_bridge,
    spawn_clipboard_monitor, spawn_ipc_server, spawn_system_tray, write_clipboard_text,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc;
use uuid::Uuid;

struct SharedRuntime {
    manager: Arc<DownloadManager>,
    bt_engine: Arc<BitTorrentEngine>,
    clipboard_enabled: Arc<AtomicBool>,
    browser_enabled: Arc<AtomicBool>,
    start_minimized: AtomicBool,
    tray_controller: TrayController,
    tray_rx: Mutex<Option<mpsc::UnboundedReceiver<TrayAction>>>,
    clip_rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
    hook_tx: mpsc::UnboundedSender<WinHookEvent>,
    hook_rx: Mutex<Option<mpsc::UnboundedReceiver<WinHookEvent>>>,
    drop_tx: mpsc::UnboundedSender<DropEvent>,
    drop_rx: Mutex<Option<mpsc::UnboundedReceiver<DropEvent>>>,
}

static SHARED: OnceLock<Arc<SharedRuntime>> = OnceLock::new();

pub fn init_shared_runtime() -> Arc<DownloadManager> {
    let shared = SHARED.get_or_init(|| {
        let data_dir = AppConfig::app_data_dir();
        let mut extra_plugins = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            let local_plugins = cwd.join("plugins");
            if local_plugins.is_dir() {
                extra_plugins.push(local_plugins);
            }
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            let beside_exe = parent.join("plugins");
            if beside_exe.is_dir() {
                extra_plugins.push(beside_exe);
            }
        }

        let bt_engine = Arc::new(BitTorrentEngine::with_data_dir(data_dir.clone()));

        let manager = DownloadManager::new(
            data_dir,
            vec![
                Arc::new(HlsEngine::new()),
                Arc::clone(&bt_engine) as Arc<dyn siphon_core::ProtocolHandler>,
                Arc::new(Ed2kEngine::new()),
                Arc::new(HttpEngine::new()),
            ],
            extra_plugins,
        )
        .expect("Failed to initialize Siphon DownloadManager");

        let clipboard_enabled = Arc::new(AtomicBool::new(true));
        let (clip_tx, clip_rx) = mpsc::unbounded_channel();
        spawn_clipboard_monitor(Arc::clone(&clipboard_enabled), clip_tx);

        let browser_enabled = Arc::new(AtomicBool::new(true));

        let (tray_tx, tray_rx) = mpsc::unbounded_channel();
        let tray_controller = spawn_system_tray(tray_tx);

        let (hook_tx, hook_rx) = mpsc::unbounded_channel();
        let (drop_tx, drop_rx) = mpsc::unbounded_channel();

        Arc::new(SharedRuntime {
            manager,
            bt_engine,
            clipboard_enabled,
            browser_enabled,
            start_minimized: AtomicBool::new(false),
            tray_controller,
            tray_rx: Mutex::new(Some(tray_rx)),
            clip_rx: Mutex::new(Some(clip_rx)),
            hook_tx,
            hook_rx: Mutex::new(Some(hook_rx)),
            drop_tx,
            drop_rx: Mutex::new(Some(drop_rx)),
        })
    });

    Arc::clone(&shared.manager)
}

pub struct SiphonApp {
    manager: Arc<DownloadManager>,
    tray: TrayController,
    clipboard_flag: Arc<AtomicBool>,
    browser_flag: Arc<AtomicBool>,

    window_id: Option<window::Id>,
    window_hwnd: Option<u64>,
    window_size: Size,
    cursor_pos: Point,
    capture_cursor_pos: Point,
    sniff_cursor_pos: Point,
    active_input: InputTarget,
    context_menu: Option<ContextMenuState>,
    drag_hovering: bool,
    system_dark: bool,

    config: AppConfig,
    tasks: Vec<DownloadTask>,
    rules: Vec<AutomationRule>,
    plugins: Vec<ExternalPlugin>,
    file_icons: HashMap<FileIconKey, (u64, image::Handle)>,
    pending_icons: HashSet<FileIconKey>,

    active_nav: NavSection,
    selected_task_id: Option<TaskId>,
    quick_input: String,
    clipboard_banner_url: Option<String>,
    toast: Option<String>,

    global_speed_bps: u64,
    active_count: usize,

    show_replace_url: bool,
    replace_url_input: String,

    modal: NewTaskModalState,
    delete_confirm: Option<DeleteConfirmState>,
    ext_install_modal: Option<ExtInstallModalState>,
    browser_ext_connected: bool,
    update_status: UpdateCheckStatus,
    auto_update_task_id: Option<TaskId>,
    app_icon_handle: image::Handle,
    author_avatar_handle: image::Handle,
    capture_popup: Option<CapturePopupState>,
    sniff_popup: Option<SniffPopupState>,
    download_popups: HashMap<window::Id, DownloadPopupState>,
    rule_draft: RuleDraft,

    // Scheduler runtime state
    sched_speed_kbps_input: String,
    sched_last_start_day: Option<(i32, u32, u32)>,
    sched_last_stop_day: Option<(i32, u32, u32)>,
    sched_speed_window_active: bool,

    // BitTorrent & Tracker page buffers
    bt_listen_port_input: String,
    bt_max_peers_input: String,
    bt_timeout_input: String,
    bt_new_tracker_input: String,
    bt_new_sub_url_input: String,
    bt_syncing_trackers: bool,
    bt_sync_status: Option<String>,

    // eD2k page buffers
    ed2k_tcp_port_input: String,
    ed2k_udp_port_input: String,
    ed2k_max_sources_input: String,
    ed2k_upload_limit_input: String,
    ed2k_quick_server_link: String,
    ed2k_new_srv_name: String,
    ed2k_new_srv_host: String,
    ed2k_new_srv_port: String,
    ed2k_syncing_met: bool,
    ed2k_met_sync_status: Option<String>,
    ed2k_server_tests: HashMap<String, ServerTestStatus>,

    // Settings text input buffers
    save_dir_input: String,
    global_speed_input: String,
    proxy_input: String,
    ua_input: String,
    accent_hex_input: String,
    popup_accent_hex_input: String,
}

impl SiphonApp {
    pub fn boot() -> (Self, Task<Message>) {
        let _ = init_shared_runtime();
        let shared = SHARED.get().expect("shared runtime initialized");
        let manager = Arc::clone(&shared.manager);
        let start_minimized = shared.start_minimized.load(Ordering::Relaxed);

        // Load initial snapshots synchronously via a temporary tiny handle or blocking read
        let (config, tasks, rules, plugins) = {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("bootstrap runtime");
            rt.block_on(async {
                let cfg = manager.config().await;
                let tasks = manager.list_tasks().await;
                let rules = manager.list_rules().await;
                let plugins = manager.list_plugins().await;
                (cfg, tasks, rules, plugins)
            })
        };

        shared
            .clipboard_enabled
            .store(config.clipboard_monitor, Ordering::Relaxed);
        shared
            .browser_enabled
            .store(config.browser_integration, Ordering::Relaxed);

        let save_dir_input = config.default_save_dir.to_string_lossy().to_string();
        let global_speed_input = (config.global_speed_limit_bps / 1024).to_string();
        let proxy_input = config.proxy_url.clone().unwrap_or_default();
        let ua_input = config.user_agent.clone();
        let accent_hex_input = config.accent_color.clone();
        let popup_accent_hex_input = config.download_popup_accent.clone();
        let sched_speed_kbps_input = config.schedule.scheduled_speed_limit_kbps.to_string();
        let bt_listen_port_input = config.bt.listen_port.to_string();
        let bt_max_peers_input = config.bt.max_peers_per_torrent.to_string();
        let bt_timeout_input = config.bt.peer_connect_timeout_secs.to_string();
        let ed2k_tcp_port_input = config.ed2k.tcp_port.to_string();
        let ed2k_udp_port_input = config.ed2k.udp_port.to_string();
        let ed2k_max_sources_input = config.ed2k.max_sources_per_file.to_string();
        let ed2k_upload_limit_input = config.ed2k.upload_limit_kbps.to_string();

        let (rgba, w, h) = siphon_platform_win::generate_siphon_icon_rgba();
        let app_icon_handle = image::Handle::from_rgba(w, h, rgba.clone());
        let app_icon = window::icon::from_rgba(rgba, w, h).ok();
        let author_avatar_handle = image::Handle::from_rgba(
            128,
            128,
            include_bytes!("../assets/author-avatar-128.rgba").as_slice(),
        );

        // Sync config.autostart with actual Windows Run registry key (e.g. if enabled by installer,
        // or refresh exe path if moved/reinstalled)
        let mut config = config;
        let reg_autostart = is_autostart_enabled();
        if config.autostart {
            let _ = set_autostart(true);
        } else if reg_autostart {
            config.autostart = true;
            let _ = set_autostart(true);
        }

        let main_win_settings = window::Settings {
            size: iced::Size::new(1340.0, 800.0),
            min_size: Some(iced::Size::new(1200.0, 680.0)),
            position: window::Position::Centered,
            decorations: false,
            transparent: true,
            resizable: true,
            visible: !start_minimized,
            exit_on_close_request: false,
            icon: app_icon,
            ..Default::default()
        };

        let (main_id, open_main) = window::open(main_win_settings);

        let mut app = Self {
            manager,
            tray: shared.tray_controller.clone(),
            clipboard_flag: Arc::clone(&shared.clipboard_enabled),
            browser_flag: Arc::clone(&shared.browser_enabled),
            window_id: Some(main_id),
            window_hwnd: None,
            window_size: Size::new(1340.0, 800.0),
            cursor_pos: Point::ORIGIN,
            capture_cursor_pos: Point::ORIGIN,
            sniff_cursor_pos: Point::ORIGIN,
            active_input: InputTarget::QuickBar,
            context_menu: None,
            drag_hovering: false,
            system_dark: is_system_dark_mode(),
            config: config.clone(),
            tasks,
            rules,
            plugins,
            file_icons: HashMap::new(),
            pending_icons: HashSet::new(),
            active_nav: NavSection::All,
            selected_task_id: None,
            quick_input: String::new(),
            clipboard_banner_url: None,
            toast: None,
            global_speed_bps: 0,
            active_count: 0,
            show_replace_url: false,
            replace_url_input: String::new(),
            modal: NewTaskModalState {
                connections: config.default_connections,
                save_dir: save_dir_input.clone(),
                speed_limit_kbps: "0".to_string(),
                ..Default::default()
            },
            delete_confirm: None,
            ext_install_modal: None,
            browser_ext_connected: false,
            update_status: UpdateCheckStatus::Idle,
            auto_update_task_id: None,
            app_icon_handle,
            author_avatar_handle,
            capture_popup: None,
            sniff_popup: None,
            download_popups: HashMap::new(),
            rule_draft: RuleDraft {
                save_dir_template: r"{base}\{domain}".to_string(),
                ..Default::default()
            },
            sched_speed_kbps_input,
            sched_last_start_day: None,
            sched_last_stop_day: None,
            sched_speed_window_active: false,
            bt_listen_port_input,
            bt_max_peers_input,
            bt_timeout_input,
            bt_new_tracker_input: String::new(),
            bt_new_sub_url_input: String::new(),
            bt_syncing_trackers: false,
            bt_sync_status: None,
            ed2k_tcp_port_input,
            ed2k_udp_port_input,
            ed2k_max_sources_input,
            ed2k_upload_limit_input,
            ed2k_quick_server_link: String::new(),
            ed2k_new_srv_name: String::new(),
            ed2k_new_srv_host: String::new(),
            ed2k_new_srv_port: "4661".to_string(),
            ed2k_syncing_met: false,
            ed2k_met_sync_status: None,
            ed2k_server_tests: HashMap::new(),
            save_dir_input,
            global_speed_input,
            proxy_input,
            ua_input,
            accent_hex_input,
            popup_accent_hex_input,
        };

        let init_window = open_main.then(|id| {
            Task::batch([
                Task::done(Message::WindowOpened(id)),
                window::raw_id::<()>(id).map(Message::WindowRawId),
            ])
        });
        let initial_keys = app.collect_all_icon_keys();
        let init_icons = app.load_file_icons(initial_keys, false);

        // Keep previously deployed browser extension files in `%APPDATA%\Siphon\browser-extension`
        // automatically synced to the latest bundled version on startup, and always ensure the
        // Native Messaging Host + `siphon://` protocol are registered so the browser can auto-launch Siphon.
        if app.manager.data_dir().join("browser-extension").is_dir() {
            let _ = deploy_browser_extension(app.manager.data_dir());
        } else {
            register_native_messaging_host(app.manager.data_dir(), &[]);
            register_siphon_protocol();
        }

        let mut boot_tasks = vec![init_window, init_icons];
        if app.config.bt.auto_sync_trackers_on_start {
            boot_tasks.push(Task::done(Message::BtSyncTrackers));
        }

        (app, Task::batch(boot_tasks))
    }

    pub fn title(&self, window_id: window::Id) -> String {
        if self
            .capture_popup
            .as_ref()
            .is_some_and(|p| p.window_id == Some(window_id))
        {
            return "Siphon 下载捕获".to_string();
        }
        if let Some(sniff) = &self.sniff_popup
            && sniff.window_id == Some(window_id)
        {
            return format!("Siphon 嗅探资源选择性下载 ({} 项)", sniff.items.len());
        }
        if let Some(dp) = self.download_popups.get(&window_id) {
            if let Some(task) = self.tasks.iter().find(|t| t.id == dp.task_id) {
                if task.status == TaskStatus::Completed {
                    return format!("[已完成] {} — Siphon", task.filename);
                }
                let pct = (task.progress_ratio() * 100.0).clamp(0.0, 100.0);
                if task.speed_bps > 0 {
                    return format!(
                        "[{pct:.1}%] {} · {} — Siphon",
                        task.filename,
                        format_speed(task.speed_bps)
                    );
                }
                return format!("[{pct:.1}%] {} — Siphon", task.filename);
            }
            return "Siphon 传输详情".to_string();
        }
        if self.active_count > 0 && self.global_speed_bps > 0 {
            format!("Siphon · ↓ {}", format_speed(self.global_speed_bps))
        } else {
            "Siphon".to_string()
        }
    }

    pub fn palette(&self) -> SiphonPalette {
        SiphonPalette::resolve(self.config.theme_mode, self.system_dark, self.config.use_mica)
            .with_custom_accent(&self.config.accent_color)
    }

    pub fn theme(&self, _window_id: window::Id) -> Theme {
        self.palette().iced_theme()
    }

    pub fn app_style(&self, _theme: &Theme) -> iced::theme::Style {
        let p = self.palette();
        iced::theme::Style {
            background_color: p.bg_window,
            text_color: p.text_primary,
        }
    }

    fn apply_backdrop_now(&self) {
        let p = self.palette();
        if let Some(hwnd) = self.window_hwnd {
            apply_win11_backdrop(hwnd, self.config.use_mica, p.is_dark);
        }
        if let Some(hwnd) = self.capture_popup.as_ref().and_then(|c| c.window_hwnd) {
            apply_win11_backdrop(hwnd, self.config.use_mica, p.is_dark);
        }
        if let Some(hwnd) = self.sniff_popup.as_ref().and_then(|s| s.window_hwnd) {
            apply_win11_backdrop(hwnd, self.config.use_mica, p.is_dark);
        }
        for dp in self.download_popups.values() {
            if let Some(hwnd) = dp.window_hwnd {
                apply_win11_backdrop(hwnd, self.config.use_mica, p.is_dark);
            }
        }
    }

    fn task_icon_key(task: &DownloadTask) -> FileIconKey {
        FileIconKey::for_task(
            &task.filename,
            &task.target_path(),
            task.status == TaskStatus::Completed,
        )
    }

    fn collect_all_icon_keys(&self) -> Vec<FileIconKey> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for t in &self.tasks {
            let key = Self::task_icon_key(t);
            if key.is_real_file() {
                let fb = key.fallback_extension();
                if seen.insert(fb.clone()) {
                    out.push(fb);
                }
            }
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
        out
    }

    fn load_file_icons(&mut self, keys: Vec<FileIconKey>, force_refresh: bool) -> Task<Message> {
        let mut expanded = Vec::new();
        for key in keys {
            if key.is_real_file() {
                let fb = key.fallback_extension();
                if !self.file_icons.contains_key(&fb) {
                    expanded.push(fb);
                }
            }
            expanded.push(key);
        }

        let mut to_fetch = Vec::new();
        let mut batch_seen = HashSet::new();
        for key in expanded {
            if !batch_seen.insert(key.clone()) {
                continue;
            }
            if !force_refresh {
                if self.file_icons.contains_key(&key) {
                    continue;
                }
                if !self.pending_icons.insert(key.clone()) {
                    continue;
                }
            } else {
                self.pending_icons.insert(key.clone());
            }
            to_fetch.push(key);
        }
        if to_fetch.is_empty() {
            return Task::none();
        }
        Task::perform(
            extract_file_icons_async(to_fetch),
            Message::FileIconsLoaded,
        )
    }

    /// Schedule immediate + delayed post-completion icon refreshes for newly finished software
    /// (`.exe` / `.msi` / `.ico` / `.lnk`) so Windows Defender scan locks or Shell icon handler
    /// latency never leave a downloaded app stuck with a generic icon.
    fn load_completed_file_icon_with_retries(&mut self, key: FileIconKey) -> Task<Message> {
        let immediate = self.load_file_icons(vec![key.clone()], true);
        if !key.is_real_file() {
            return immediate;
        }
        let k1 = key.clone();
        let retry_400ms = Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                extract_file_icons_async(vec![k1]).await
            },
            Message::FileIconsLoaded,
        );
        let retry_1500ms = Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                extract_file_icons_async(vec![key]).await
            },
            Message::FileIconsLoaded,
        );
        Task::batch([immediate, retry_400ms, retry_1500ms])
    }

    fn evaluate_capture_defaults(
        &self,
        url: &str,
        initial_filename: Option<&str>,
        mime: Option<&str>,
        size_bytes: Option<u64>,
        source: TaskSource,
    ) -> (String, String, u32, u64, Option<String>) {
        let parsed = url::Url::parse(url).ok();
        let fallback = initial_filename
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| infer_filename_from_raw_url(url, parsed.as_ref()));
        let filename = sanitize_filename(&fallback);
        let category = FileCategory::from_filename_and_mime(&filename, mime);
        let domain = parsed
            .as_ref()
            .and_then(|u| u.host_str())
            .unwrap_or("direct")
            .to_string();

        let engine = RuleEngine::new(self.rules.clone());
        let matched = engine.evaluate(&RuleEvalInput {
            url,
            domain: &domain,
            filename: &filename,
            mime,
            size_bytes,
            category,
            source,
            base_save_dir: &self.config.default_save_dir,
        });

        let save_dir = matched
            .save_dir
            .unwrap_or_else(|| self.config.default_save_dir.clone())
            .to_string_lossy()
            .to_string();
        let connections = matched
            .connections_override
            .unwrap_or(self.config.default_connections)
            .clamp(1, 32);
        let speed_limit_bps = matched.speed_limit_bps.unwrap_or(0);
        let rule_name = matched.matched_rules.first().cloned();

        (filename, save_dir, connections, speed_limit_bps, rule_name)
    }

    fn open_or_update_capture_popup(
        &mut self,
        payload: BrowserCapturePayload,
        source: TaskSource,
    ) -> Task<Message> {
        let url = payload.url.trim().to_string();
        if url.is_empty() {
            return Task::none();
        }
        let headers = payload.merged_headers();
        let (filename, save_dir, connections, speed_limit_bps, matched_rule_name) = self
            .evaluate_capture_defaults(
                &url,
                payload.filename.as_deref(),
                payload.mime.as_deref(),
                payload.file_size,
                source,
            );

        let icon_key = FileIconKey::for_task(&filename, Path::new(""), false);
        let icon_task = self.load_file_icons(vec![icon_key], false);

        let existing_win = self.capture_popup.as_ref().and_then(|p| p.window_id);
        let existing_hwnd = self.capture_popup.as_ref().and_then(|p| p.window_hwnd);

        let (popup_win_id, win_task) = if let Some(pid) = existing_win {
            if let Some(hwnd) = existing_hwnd {
                show_window_native(hwnd);
            }
            (Some(pid), window::gain_focus(pid))
        } else {
            let (rgba, w, h) = siphon_platform_win::generate_siphon_icon_rgba();
            let app_icon = window::icon::from_rgba(rgba, w, h).ok();
            let popup_settings = window::Settings {
                size: iced::Size::new(620.0, 404.0),
                min_size: Some(iced::Size::new(620.0, 404.0)),
                max_size: Some(iced::Size::new(620.0, 404.0)),
                position: window::Position::Centered,
                decorations: false,
                transparent: true,
                resizable: false,
                visible: true,
                level: window::Level::AlwaysOnTop,
                exit_on_close_request: false,
                icon: app_icon,
                ..Default::default()
            };
            let (pid, open_popup) = window::open(popup_settings);
            let setup = open_popup.then(|id| {
                Task::batch([
                    Task::done(Message::CaptureWindowOpened(id)),
                    window::raw_id::<()>(id).map(Message::CaptureWindowRawId),
                    window::gain_focus(id),
                ])
            });
            (Some(pid), setup)
        };

        self.capture_popup = Some(CapturePopupState {
            window_id: popup_win_id,
            window_hwnd: existing_hwnd,
            source,
            url: url.clone(),
            filename,
            save_dir,
            headers: headers.clone(),
            connections,
            speed_limit_bps,
            known_size: payload.file_size,
            supports_range: None,
            matched_rule_name,
            probing: true,
            probe_error: None,
            dont_ask_again: false,
            filename_edited_by_user: false,
            save_dir_edited_by_user: false,
        });
        self.active_input = InputTarget::CaptureFilename;
        self.context_menu = None;

        let mgr = Arc::clone(&self.manager);
        let url_for_probe = url.clone();
        let probe_task = Task::perform(
            async move {
                let res = mgr
                    .probe_url(&url_for_probe, &headers, source)
                    .await
                    .map_err(|e| e.to_string());
                (url_for_probe, res)
            },
            |(u, r)| Message::CaptureProbeFinished(u, r),
        );

        Task::batch([win_task, icon_task, probe_task])
    }

    /// Open or live-update the standalone Browser Resource Sniff Manager popup window.
    fn open_or_update_sniff_popup(&mut self, payload: BrowserSniffPayload) -> Task<Message> {
        let headers = payload.merged_headers();
        let incoming_page_url = payload.page_url.clone().unwrap_or_default();
        let incoming_page_title = payload.page_title.clone().unwrap_or_default();
        let has_non_image = payload
            .items
            .iter()
            .any(|it| !it.kind.eq_ignore_ascii_case("image"));

        let build_entry = |id: usize, it: siphon_platform_win::SniffedResourceItem| -> SniffEntry {
            let parsed = url::Url::parse(&it.url).ok();
            let raw_name = it
                .filename
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| infer_filename_from_raw_url(&it.url, parsed.as_ref()));
            let filename = sanitize_filename(&raw_name);
            let kind = if it.kind.trim().is_empty() {
                "file".to_string()
            } else {
                it.kind.trim().to_ascii_lowercase()
            };
            let selected = if has_non_image {
                kind != "image"
            } else {
                true
            };
            SniffEntry {
                id,
                url: it.url,
                filename,
                kind,
                label: it.label.unwrap_or_default(),
                mime: it.mime,
                file_size: it.file_size,
                selected,
            }
        };

        if let Some(existing) = &mut self.sniff_popup {
            let same_page = !incoming_page_url.is_empty()
                && existing.page_url == incoming_page_url;
            if !incoming_page_title.is_empty() {
                existing.page_title = incoming_page_title;
            }
            if !headers.is_empty() {
                existing.headers = headers;
            }
            if same_page {
                for it in payload.items {
                    if let Some(slot) = existing.items.iter_mut().find(|e| e.url == it.url) {
                        if slot.file_size.is_none() && it.file_size.is_some() {
                            slot.file_size = it.file_size;
                        }
                        if slot.mime.is_none() && it.mime.is_some() {
                            slot.mime = it.mime;
                        }
                    } else {
                        let id = existing.next_entry_id;
                        existing.next_entry_id += 1;
                        let entry = build_entry(id, it);
                        if matches!(entry.kind.as_str(), "hls" | "video" | "audio") {
                            existing.items.insert(0, entry);
                        } else {
                            existing.items.push(entry);
                        }
                    }
                }
            } else {
                existing.page_url = incoming_page_url.clone();
                if !incoming_page_url.is_empty() {
                    existing.page_url_input = incoming_page_url;
                }
                existing.sniff_error = None;
                existing.filter = SniffFilterKind::All;
                existing.search.clear();
                existing.items.clear();
                for it in payload.items {
                    if existing.items.iter().any(|e| e.url == it.url) {
                        continue;
                    }
                    let id = existing.next_entry_id;
                    existing.next_entry_id += 1;
                    existing.items.push(build_entry(id, it));
                }
            }

            if payload.bring_to_front {
                if let Some(hwnd) = existing.window_hwnd {
                    show_window_native(hwnd);
                }
                if let Some(wid) = existing.window_id {
                    return window::gain_focus(wid);
                }
            }
            return Task::none();
        }

        let mut entries = Vec::new();
        let mut next_id = 1usize;
        for it in payload.items {
            if entries.iter().any(|e: &SniffEntry| e.url == it.url) {
                continue;
            }
            entries.push(build_entry(next_id, it));
            next_id += 1;
        }

        let (rgba, w, h) = siphon_platform_win::generate_siphon_icon_rgba();
        let app_icon = window::icon::from_rgba(rgba, w, h).ok();
        let settings = window::Settings {
            size: iced::Size::new(780.0, 580.0),
            min_size: Some(iced::Size::new(680.0, 480.0)),
            position: window::Position::Centered,
            decorations: false,
            transparent: true,
            resizable: true,
            visible: true,
            level: window::Level::AlwaysOnTop,
            exit_on_close_request: false,
            icon: app_icon,
            ..Default::default()
        };
        let (wid, open_task) = window::open(settings);
        self.sniff_popup = Some(SniffPopupState {
            window_id: Some(wid),
            window_hwnd: None,
            page_title: incoming_page_title,
            page_url_input: incoming_page_url.clone(),
            page_url: incoming_page_url,
            sniffing_page: false,
            probing_sizes: false,
            sniff_error: None,
            headers,
            items: entries,
            next_entry_id: next_id,
            filter: SniffFilterKind::All,
            search: String::new(),
            save_dir: self.config.default_save_dir.to_string_lossy().to_string(),
            connections: self.config.default_connections.clamp(1, 32),
        });
        self.context_menu = None;

        open_task.then(|id| {
            Task::batch([
                Task::done(Message::SniffWindowOpened(id)),
                window::raw_id::<()>(id).map(Message::SniffWindowRawId),
                window::gain_focus(id),
            ])
        })
    }

    /// Open or focus the standalone download progress detail popup window for `task_id`.
    fn open_download_detail_popup(&mut self, task_id: TaskId) -> Task<Message> {
        if let Some(existing) = self
            .download_popups
            .values()
            .find(|p| p.task_id == task_id)
        {
            return window::gain_focus(existing.window_id);
        }

        let (rgba, w, h) = siphon_platform_win::generate_siphon_icon_rgba();
        let app_icon = window::icon::from_rgba(rgba, w, h).ok();
        let settings = window::Settings {
            size: iced::Size::new(600.0, 460.0),
            min_size: Some(iced::Size::new(600.0, 460.0)),
            max_size: Some(iced::Size::new(600.0, 460.0)),
            position: window::Position::Centered,
            decorations: false,
            transparent: true,
            resizable: false,
            visible: true,
            exit_on_close_request: false,
            icon: app_icon,
            ..Default::default()
        };
        let (win_id, open_task) = window::open(settings);
        self.download_popups.insert(
            win_id,
            DownloadPopupState {
                window_id: win_id,
                window_hwnd: None,
                task_id,
                auto_close_on_complete: false,
            },
        );
        open_task.then(|id| {
            Task::batch([
                window::raw_id::<()>(id).map(move |hwnd| Message::DownloadPopupRawId(id, hwnd)),
                window::gain_focus(id),
            ])
        })
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowOpened(id) => {
                if self
                    .capture_popup
                    .as_ref()
                    .is_some_and(|p| p.window_id == Some(id))
                {
                    return window::raw_id::<()>(id).map(Message::CaptureWindowRawId);
                }
                if self
                    .sniff_popup
                    .as_ref()
                    .is_some_and(|p| p.window_id == Some(id))
                {
                    return window::raw_id::<()>(id).map(Message::SniffWindowRawId);
                }
                if self.download_popups.contains_key(&id) {
                    return window::raw_id::<()>(id)
                        .map(move |hwnd| Message::DownloadPopupRawId(id, hwnd));
                }
                self.window_id = Some(id);
                return window::raw_id::<()>(id).map(Message::WindowRawId);
            }
            Message::WindowRawId(hwnd) => {
                self.window_hwnd = Some(hwnd);
                self.apply_backdrop_now();
                if let Some(shared) = SHARED.get() {
                    install_window_hooks(hwnd, shared.hook_tx.clone());
                    install_url_drop_target(hwnd, shared.drop_tx.clone());
                }
            }
            Message::CaptureWindowOpened(id) => {
                if let Some(popup) = &mut self.capture_popup {
                    popup.window_id = Some(id);
                }
                return window::raw_id::<()>(id).map(Message::CaptureWindowRawId);
            }
            Message::CaptureWindowRawId(hwnd) => {
                if let Some(popup) = &mut self.capture_popup {
                    popup.window_hwnd = Some(hwnd);
                }
                self.apply_backdrop_now();
                show_window_native(hwnd);
                if let Some(shared) = SHARED.get() {
                    install_window_hooks(hwnd, shared.hook_tx.clone());
                }
                return operation::focus(views::capture_popup::capture_filename_id());
            }
            Message::WindowResized(id, size) => {
                if self.window_id == Some(id) {
                    self.window_size = size;
                }
                self.context_menu = None;
            }
            Message::WindowFocused(_id) => {
                let dark = is_system_dark_mode();
                if dark != self.system_dark {
                    self.system_dark = dark;
                    self.apply_backdrop_now();
                }
                let keys = self.collect_all_icon_keys();
                return self.load_file_icons(keys, true);
            }
            Message::WinHook(ev) => match ev {
                WinHookEvent::Paste => return self.update(Message::PasteIntoActiveInput),
                WinHookEvent::ShellIconsChanged => {
                    let dark = is_system_dark_mode();
                    if dark != self.system_dark {
                        self.system_dark = dark;
                        self.apply_backdrop_now();
                    }
                    let keys = self.collect_all_icon_keys();
                    return self.load_file_icons(keys, true);
                }
            },
            Message::FileIconsLoaded(results) => {
                for (key, bmp) in results {
                    self.pending_icons.remove(&key);
                    match bmp {
                        Some(bmp) => {
                            let unchanged = self
                                .file_icons
                                .get(&key)
                                .is_some_and(|(h, _)| *h == bmp.hash);
                            if !unchanged {
                                let handle =
                                    image::Handle::from_rgba(bmp.width, bmp.height, bmp.rgba);
                                self.file_icons.insert(key, (bmp.hash, handle));
                            }
                        }
                        None => {
                            // Never wipe an already-loaded icon on a transient Shell/AV failure;
                            // only evict a RealFile icon when the file itself no longer exists on disk.
                            if let FileIconKey::RealFile(ref path) = key
                                && !path.is_file()
                            {
                                self.file_icons.remove(&key);
                            }
                        }
                    }
                }
            }
            Message::CursorMoved(id, pos) => {
                if self
                    .capture_popup
                    .as_ref()
                    .is_some_and(|p| p.window_id == Some(id))
                {
                    self.capture_cursor_pos = pos;
                } else if self
                    .sniff_popup
                    .as_ref()
                    .is_some_and(|p| p.window_id == Some(id))
                {
                    self.sniff_cursor_pos = pos;
                } else {
                    self.cursor_pos = pos;
                }
            }
            Message::WindowCloseRequested(id) => {
                if self
                    .capture_popup
                    .as_ref()
                    .is_some_and(|p| p.window_id == Some(id))
                {
                    self.capture_popup = None;
                    return window::close(id);
                }
                if self
                    .sniff_popup
                    .as_ref()
                    .is_some_and(|p| p.window_id == Some(id))
                {
                    self.sniff_popup = None;
                    return window::close(id);
                }
                if self.download_popups.remove(&id).is_some() {
                    return window::close(id);
                }
                if self.config.minimize_to_tray {
                    return window::set_mode(id, window::Mode::Hidden);
                } else {
                    return iced::exit();
                }
            }
            Message::WindowClosed(id) => {
                if self
                    .capture_popup
                    .as_ref()
                    .is_some_and(|p| p.window_id == Some(id))
                {
                    self.capture_popup = None;
                }
                if self
                    .sniff_popup
                    .as_ref()
                    .is_some_and(|p| p.window_id == Some(id))
                {
                    self.sniff_popup = None;
                }
                self.download_popups.remove(&id);
            }
            Message::DragWindow => {
                if let Some(id) = self.window_id {
                    return window::drag(id);
                }
            }
            Message::MinimizeWindow => {
                if let Some(id) = self.window_id {
                    return window::minimize(id, true);
                }
            }
            Message::ToggleMaximizeWindow => {
                if let Some(id) = self.window_id {
                    return window::toggle_maximize(id);
                }
            }
            Message::CloseWindow => {
                if let Some(id) = self.window_id {
                    if self.config.minimize_to_tray {
                        return window::set_mode(id, window::Mode::Hidden);
                    } else {
                        return iced::exit();
                    }
                }
            }
            Message::FileDropped(path) => {
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("torrent"))
                    && let Ok(file_url) = url::Url::from_file_path(&path)
                {
                    return self.update(Message::OpenNewTaskModal(Some(file_url.to_string())));
                }
                if let Some(url) = extract_url_from_dropped_file(&path) {
                    return self.update(Message::OpenNewTaskModal(Some(url)));
                } else {
                    self.toast = Some(format!(
                        "未在拖入文件 {} 中检测到 HTTP/HTTPS/Magnet 链接或 .torrent 种子",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }
            }
            Message::ShortcutNewTask => {
                return self.update(Message::OpenNewTaskModal(None));
            }
            Message::ShortcutFocusQuickBar => {
                self.active_input = InputTarget::QuickBar;
                return operation::focus(views::titlebar::quick_bar_id());
            }
            Message::ShortcutEscape => {
                if self.context_menu.is_some() {
                    self.context_menu = None;
                } else if self.ext_install_modal.is_some() {
                    self.ext_install_modal = None;
                } else if self.capture_popup.is_some() {
                    return self.update(Message::CaptureCancel);
                } else if self.sniff_popup.is_some() {
                    return self.update(Message::SniffClose);
                } else if self.delete_confirm.is_some() {
                    self.delete_confirm = None;
                } else if self.modal.open {
                    self.modal.open = false;
                    self.active_input = InputTarget::QuickBar;
                } else if self.rule_draft.editing_id.is_some() {
                    return self.update(Message::RuleEditCancel);
                } else if self.selected_task_id.is_some() {
                    self.selected_task_id = None;
                }
            }
            Message::FocusInputTarget(target) => {
                self.active_input = target;
                self.context_menu = None;
            }
            Message::OpenInputContextMenu(target) => {
                self.active_input = target;
                let position = if matches!(
                    target,
                    InputTarget::CaptureFilename | InputTarget::CaptureSaveDir
                ) {
                    self.capture_cursor_pos
                } else if matches!(
                    target,
                    InputTarget::SniffPageUrl | InputTarget::SniffSearch | InputTarget::SniffSaveDir
                ) {
                    self.sniff_cursor_pos
                } else {
                    self.cursor_pos
                };
                self.context_menu = Some(ContextMenuState { target, position });
            }
            Message::CloseContextMenu => {
                self.context_menu = None;
            }
            Message::PasteIntoActiveInput => {
                let target = if (self.capture_popup.is_some()
                    && matches!(
                        self.active_input,
                        InputTarget::CaptureFilename | InputTarget::CaptureSaveDir
                    ))
                    || (self.sniff_popup.is_some()
                        && matches!(
                            self.active_input,
                            InputTarget::SniffPageUrl
                                | InputTarget::SniffSearch
                                | InputTarget::SniffSaveDir
                        ))
                {
                    self.active_input
                } else if self.modal.open {
                    match self.active_input {
                        InputTarget::ModalUrl
                        | InputTarget::ModalFilename
                        | InputTarget::ModalSaveDir
                        | InputTarget::ModalSpeedLimit
                        | InputTarget::ModalSha256
                        | InputTarget::ModalBatchUrls
                        | InputTarget::ModalBatchFilter => self.active_input,
                        _ => {
                            if self.modal.batch_mode {
                                InputTarget::ModalBatchUrls
                            } else {
                                InputTarget::ModalUrl
                            }
                        }
                    }
                } else {
                    self.active_input
                };
                return self.paste_clipboard_into(target, false);
            }
            Message::ContextPaste(target) => {
                self.context_menu = None;
                return self.paste_clipboard_into(target, false);
            }
            Message::ContextPasteAndGo(target) => {
                self.context_menu = None;
                return self.paste_clipboard_into(target, true);
            }
            Message::ContextCopy(target) => {
                self.context_menu = None;
                let text = self.get_input_value(target).to_string();
                if !text.is_empty() {
                    let _ = write_clipboard_text(&text);
                }
            }
            Message::ContextClear(target) => {
                self.context_menu = None;
                self.set_input_value(target, String::new());
            }

            // Background engine events
            Message::EngineEvent(ev) => match ev {
                TaskEvent::TaskAdded(task) => {
                    let icon_key = Self::task_icon_key(&task);
                    self.tasks.retain(|t| t.id != task.id);
                    let id = task.id;
                    self.tasks.insert(0, task);
                    if self.selected_task_id.is_none() {
                        self.selected_task_id = Some(id);
                    }
                    return self.load_file_icons(vec![icon_key], false);
                }
                TaskEvent::TaskUpdated(updated) => {
                    let icon_key = Self::task_icon_key(&updated);
                    let became_completed = updated.status == TaskStatus::Completed
                        && self
                            .tasks
                            .iter()
                            .find(|t| t.id == updated.id)
                            .is_none_or(|prev| prev.status != TaskStatus::Completed);
                    if let Some(slot) = self.tasks.iter_mut().find(|t| t.id == updated.id) {
                        let prev_hist = std::mem::take(&mut slot.speed_history);
                        *slot = updated;
                        if slot.speed_history.is_empty() {
                            slot.speed_history = prev_hist;
                        }
                    } else {
                        self.tasks.insert(0, updated);
                    }
                    if became_completed {
                        return self.load_completed_file_icon_with_retries(icon_key);
                    }
                    return self.load_file_icons(vec![icon_key], false);
                }
                TaskEvent::TaskProgress {
                    task_id,
                    downloaded_bytes,
                    total_size,
                    speed_bps,
                    eta_secs,
                    segments,
                } => {
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                        if task.status == TaskStatus::Probing {
                            task.status = TaskStatus::Downloading;
                        }
                        task.downloaded_bytes = downloaded_bytes;
                        if total_size.is_some() {
                            task.total_size = total_size;
                        }
                        task.speed_bps = speed_bps;
                        task.eta_secs = eta_secs;
                        task.segments = segments;
                        task.push_speed_sample(speed_bps);
                    }
                }
                TaskEvent::TaskCompleted {
                    task_id,
                    filename,
                    target_path,
                } => {
                    // Check 1: In-app 1-click auto-update installer finished -> launch silent upgrade & exit!
                    if self.auto_update_task_id == Some(task_id) {
                        self.auto_update_task_id = None;
                        match launch_silent_installer_update(&target_path) {
                            Ok(()) => {
                                return iced::exit();
                            }
                            Err(e) => {
                                self.toast = Some(format!("启动自动更新安装程序失败: {e}"));
                            }
                        }
                    }

                    self.toast = Some(format!("下载完成: {filename}"));
                    let key = FileIconKey::for_task(&filename, &target_path, true);
                    let icon_task = self.load_completed_file_icon_with_retries(key);
                    let auto_close_ids: Vec<window::Id> = self
                        .download_popups
                        .values()
                        .filter(|p| p.task_id == task_id && p.auto_close_on_complete)
                        .map(|p| p.window_id)
                        .collect();
                    let mut batch = vec![icon_task];
                    for wid in auto_close_ids {
                        self.download_popups.remove(&wid);
                        batch.push(window::close(wid));
                    }

                    // Check 2: Queue-complete action when all remaining active/queued tasks are finished
                    let action = self.config.schedule.on_queue_complete;
                    if action != QueueCompleteAction::None {
                        let any_remaining = self.tasks.iter().any(|t| {
                            t.id != task_id
                                && (t.status.is_active() || t.status == TaskStatus::Queued)
                        });
                        if !any_remaining {
                            let script = self.config.schedule.post_queue_script.clone();
                            if self.config.schedule.one_shot_complete_action {
                                self.config.schedule.on_queue_complete = QueueCompleteAction::None;
                                let _ = self.config.save(self.manager.data_dir());
                                batch.push(self.save_config_async());
                            }
                            if action == QueueCompleteAction::ExitApp {
                                return iced::exit();
                            }
                            match execute_power_action(action, &script) {
                                Ok(()) => {
                                    let desc = match action {
                                        QueueCompleteAction::Sleep => "系统睡眠",
                                        QueueCompleteAction::Hibernate => "系统休眠",
                                        QueueCompleteAction::Shutdown => "30 秒后关机",
                                        QueueCompleteAction::RunScript => "自定义脚本",
                                        _ => "完成动作",
                                    };
                                    self.toast = Some(format!("全部任务已完成，已触发：{desc}"));
                                }
                                Err(e) => {
                                    self.toast = Some(format!("执行完成后动作失败: {e}"));
                                }
                            }
                        }
                    }
                    return Task::batch(batch);
                }
                TaskEvent::TaskFailed { task_id, error, .. } => {
                    if self.auto_update_task_id == Some(task_id) {
                        self.auto_update_task_id = None;
                    }
                    self.toast = Some(format!("下载中断: {error}"));
                }
                TaskEvent::TaskRemoved(id) => {
                    self.tasks.retain(|t| t.id != id);
                    if self.selected_task_id == Some(id) {
                        self.selected_task_id = None;
                    }
                    let popups_to_close: Vec<window::Id> = self
                        .download_popups
                        .values()
                        .filter(|p| p.task_id == id)
                        .map(|p| p.window_id)
                        .collect();
                    if !popups_to_close.is_empty() {
                        let mut batch = Vec::new();
                        for wid in popups_to_close {
                            self.download_popups.remove(&wid);
                            batch.push(window::close(wid));
                        }
                        return Task::batch(batch);
                    }
                }
                TaskEvent::GlobalSpeedTick {
                    total_speed_bps,
                    active_count,
                } => {
                    self.global_speed_bps = total_speed_bps;
                    self.active_count = active_count;
                    if active_count > 0 {
                        self.tray.set_tooltip(format!(
                            "Siphon — 下载中 ({active_count}) · {}",
                            format_speed(total_speed_bps)
                        ));
                    } else {
                        self.tray.set_tooltip("Siphon — 待机中");
                    }
                }
                TaskEvent::LogEntry { task_id, entry } => {
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                        task.logs.push(entry);
                        if task.logs.len() > 80 {
                            let d = task.logs.len() - 80;
                            task.logs.drain(0..d);
                        }
                    }
                }
            },

            // System tray actions
            Message::TrayEvent(act) => match act {
                TrayAction::ShowWindow => {
                    if let Some(hwnd) = self.window_hwnd {
                        show_window_native(hwnd);
                    }
                    if let Some(id) = self.window_id {
                        return Task::batch([
                            window::set_mode(id, window::Mode::Windowed),
                            window::minimize(id, false),
                            window::gain_focus(id),
                        ]);
                    }
                }
                TrayAction::NewTask => {
                    if let Some(hwnd) = self.window_hwnd {
                        show_window_native(hwnd);
                    }
                    let open_task = self.update(Message::OpenNewTaskModal(None));
                    if let Some(id) = self.window_id {
                        return Task::batch([
                            window::set_mode(id, window::Mode::Windowed),
                            window::minimize(id, false),
                            window::gain_focus(id),
                            open_task,
                        ]);
                    }
                    return open_task;
                }
                TrayAction::PauseAll => return self.update(Message::PauseAll),
                TrayAction::ResumeAll => return self.update(Message::ResumeAll),
                TrayAction::Quit => return iced::exit(),
            },

            // Named Pipe IPC events (secondary launch or Chrome/Edge extension)
            Message::IpcEvent(cmd) => match cmd {
                IpcCommand::ShowWindow => {
                    if let Some(hwnd) = self.window_hwnd {
                        show_window_native(hwnd);
                    }
                    if let Some(id) = self.window_id {
                        return Task::batch([
                            window::set_mode(id, window::Mode::Windowed),
                            window::minimize(id, false),
                            window::gain_focus(id),
                        ]);
                    }
                }
                IpcCommand::AddDownload {
                    url,
                    filename,
                    headers,
                    from_browser,
                } => {
                    if from_browser {
                        if !self.config.browser_integration {
                            return Task::none();
                        }
                        if self.config.browser_capture_popup {
                            let payload = BrowserCapturePayload {
                                url,
                                filename,
                                headers,
                                ..Default::default()
                            };
                            return self.open_or_update_capture_popup(payload, TaskSource::Browser);
                        }
                    }
                    let mgr = Arc::clone(&self.manager);
                    let req = NewTaskRequest {
                        url,
                        filename,
                        save_dir: None,
                        max_connections: None,
                        speed_limit_bps: None,
                        headers,
                        checksum: None,
                        source: if from_browser {
                            TaskSource::Browser
                        } else {
                            TaskSource::Manual
                        },
                        auto_start: true,
                    };
                    let add_task = Task::perform(
                        async move { mgr.add_task(req).await.map(|_| ()).map_err(|e| e.to_string()) },
                        Message::TaskOpFinished,
                    );
                    if !from_browser
                        && let Some(id) = self.window_id
                    {
                        return Task::batch([
                            window::set_mode(id, window::Mode::Windowed),
                            window::gain_focus(id),
                            add_task,
                        ]);
                    }
                    return add_task;
                }
                IpcCommand::BrowserCapture(payload) => {
                    self.browser_ext_connected = true;
                    if !self.config.browser_integration {
                        return Task::none();
                    }
                    if !payload.silent && self.config.browser_capture_popup {
                        return self.open_or_update_capture_popup(payload, TaskSource::Browser);
                    }
                    let headers = payload.merged_headers();
                    let label = payload
                        .filename
                        .clone()
                        .unwrap_or_else(|| payload.url.clone());
                    let mgr = Arc::clone(&self.manager);
                    let req = NewTaskRequest {
                        url: payload.url,
                        filename: payload.filename,
                        save_dir: None,
                        max_connections: None,
                        speed_limit_bps: None,
                        headers,
                        checksum: None,
                        source: TaskSource::Browser,
                        auto_start: true,
                    };
                    if payload.silent {
                        self.toast = Some(format!("已从浏览器右键提交后台任务: {label}"));
                    }
                    return Task::perform(
                        async move { mgr.add_task(req).await.map(|_| ()).map_err(|e| e.to_string()) },
                        Message::TaskOpFinished,
                    );
                }
                IpcCommand::BrowserSniff(payload) => {
                    self.browser_ext_connected = true;
                    return self.open_or_update_sniff_popup(payload);
                }
                IpcCommand::ExtensionHandshake { extension_id } => {
                    self.browser_ext_connected = true;
                    register_native_messaging_host(self.manager.data_dir(), &[extension_id]);
                }
                IpcCommand::Ping => {
                    self.browser_ext_connected = true;
                }
            },

            Message::DropEvent(ev) => match ev {
                DropEvent::Hovering(v) => {
                    self.drag_hovering = v;
                }
                DropEvent::DroppedUrls(urls) => {
                    self.drag_hovering = false;
                    return self.handle_dropped_urls(urls);
                }
                DropEvent::DroppedFiles(paths) => {
                    self.drag_hovering = false;
                    let mut urls = Vec::new();
                    for path in &paths {
                        if path
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("torrent"))
                            && let Ok(file_url) = url::Url::from_file_path(path)
                        {
                            urls.push(file_url.to_string());
                            continue;
                        }
                        if let Ok(raw) = std::fs::read_to_string(path) {
                            if let Some(u) = extract_url_from_dropped_file(path) {
                                urls.push(u);
                            } else {
                                urls.extend(extract_urls_from_text(&raw));
                            }
                        }
                    }
                    if urls.is_empty() {
                        self.toast =
                            Some("未在拖入文件中检测到有效的 HTTP/HTTPS/Magnet 链接或 .torrent 种子".to_string());
                    } else {
                        return self.handle_dropped_urls(urls);
                    }
                }
            },

            // Clipboard monitor banner or direct floating capture popup
            Message::ClipboardUrlDetected(url) => {
                if self.config.clipboard_capture_popup {
                    if self
                        .capture_popup
                        .as_ref()
                        .is_some_and(|p| p.url == url)
                    {
                        return Task::none();
                    }
                    let payload = BrowserCapturePayload {
                        url,
                        ..Default::default()
                    };
                    return self.open_or_update_capture_popup(payload, TaskSource::Clipboard);
                }
                self.clipboard_banner_url = Some(url);
            }
            Message::CaptureDragWindow => {
                if let Some(pid) = self.capture_popup.as_ref().and_then(|p| p.window_id) {
                    return window::drag(pid);
                }
            }
            Message::CaptureFilenameChanged(v) => {
                let mut icon_key_opt = None;
                if let Some(popup) = &mut self.capture_popup {
                    popup.filename = v;
                    popup.filename_edited_by_user = true;
                    icon_key_opt = Some(FileIconKey::for_task(
                        &popup.filename,
                        Path::new(""),
                        false,
                    ));
                }
                if let Some(k) = icon_key_opt {
                    return self.load_file_icons(vec![k], false);
                }
            }
            Message::CaptureSaveDirChanged(v) => {
                if let Some(popup) = &mut self.capture_popup {
                    popup.save_dir = v;
                    popup.save_dir_edited_by_user = true;
                }
            }
            Message::CaptureBrowseDir => {
                let start = self
                    .capture_popup
                    .as_ref()
                    .map(|p| PathBuf::from(&p.save_dir))
                    .unwrap_or_else(|| self.config.default_save_dir.clone());
                return Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new();
                        if start.is_dir() {
                            dlg = dlg.set_directory(start);
                        }
                        dlg.pick_folder().await.map(|h| h.path().to_path_buf())
                    },
                    Message::CaptureDirPicked,
                );
            }
            Message::CaptureDirPicked(Some(dir)) => {
                if let Some(popup) = &mut self.capture_popup {
                    popup.save_dir = dir.to_string_lossy().to_string();
                    popup.save_dir_edited_by_user = true;
                }
            }
            Message::CaptureDirPicked(None) => {}
            Message::CaptureToggleDontAsk(v) => {
                if let Some(popup) = &mut self.capture_popup {
                    popup.dont_ask_again = v;
                }
            }
            Message::CaptureProbeFinished(probed_url, res) => {
                let mut icon_to_load = None;
                if let Some(popup) = &mut self.capture_popup
                    && popup.url == probed_url
                {
                    popup.probing = false;
                    match res {
                        Ok(preview) => {
                            popup.probe_error = None;
                            if preview.metadata.content_length.is_some() {
                                popup.known_size = preview.metadata.content_length;
                            }
                            popup.supports_range = Some(preview.metadata.supports_range);
                            popup.matched_rule_name = preview.matched_rule_name;
                            popup.connections = preview.suggested_connections;
                            popup.speed_limit_bps = preview.suggested_speed_limit_bps;
                            for (k, v) in preview.metadata.resolved_headers {
                                popup.headers.entry(k).or_insert(v);
                            }
                            if !popup.filename_edited_by_user
                                && !preview.metadata.filename.is_empty()
                            {
                                popup.filename = preview.metadata.filename;
                            }
                            if !popup.save_dir_edited_by_user {
                                popup.save_dir =
                                    preview.suggested_save_dir.to_string_lossy().to_string();
                            }
                            icon_to_load = Some(FileIconKey::for_task(
                                &popup.filename,
                                Path::new(""),
                                false,
                            ));
                        }
                        Err(e) => {
                            popup.probe_error = Some(e);
                        }
                    }
                }
                if let Some(k) = icon_to_load {
                    return self.load_file_icons(vec![k], false);
                }
            }
            Message::CaptureCancel => {
                self.context_menu = None;
                if let Some(popup) = self.capture_popup.take()
                    && let Some(pid) = popup.window_id
                {
                    return window::close(pid);
                }
            }
            Message::CaptureSubmit(action) => {
                self.context_menu = None;
                if let Some(popup) = self.capture_popup.take() {
                    let mut batch = Vec::new();
                    if let Some(pid) = popup.window_id {
                        batch.push(window::close(pid));
                    }
                    if popup.dont_ask_again {
                        if popup.source == TaskSource::Browser {
                            self.config.browser_capture_popup = false;
                        } else if popup.source == TaskSource::Clipboard {
                            self.config.clipboard_capture_popup = false;
                        }
                        batch.push(self.save_config_async());
                    }

                    let filename = {
                        let f = popup.filename.trim();
                        if f.is_empty() {
                            None
                        } else {
                            Some(f.to_string())
                        }
                    };
                    let save_dir = {
                        let d = popup.save_dir.trim();
                        if d.is_empty() {
                            None
                        } else {
                            Some(PathBuf::from(d))
                        }
                    };
                    let auto_start = matches!(
                        action,
                        CaptureAction::Background | CaptureAction::StartWithDialog
                    );
                    let req = NewTaskRequest {
                        url: popup.url,
                        filename,
                        save_dir,
                        max_connections: Some(popup.connections),
                        speed_limit_bps: Some(popup.speed_limit_bps),
                        headers: popup.headers,
                        checksum: None,
                        source: popup.source,
                        auto_start,
                    };
                    let mgr = Arc::clone(&self.manager);
                    if action == CaptureAction::StartWithDialog {
                        batch.push(Task::perform(
                            async move { mgr.add_task(req).await.map_err(|e| e.to_string()) },
                            Message::CaptureStartedWithDialog,
                        ));
                    } else {
                        batch.push(Task::perform(
                            async move { mgr.add_task(req).await.map(|_| ()).map_err(|e| e.to_string()) },
                            Message::TaskOpFinished,
                        ));
                    }
                    return Task::batch(batch);
                }
            }
            Message::CaptureStartedWithDialog(res) => match res {
                Ok(task) => {
                    let task_id = task.id;
                    let icon_key = Self::task_icon_key(&task);
                    self.tasks.retain(|t| t.id != task_id);
                    self.tasks.insert(0, task);
                    if self.selected_task_id.is_none() {
                        self.selected_task_id = Some(task_id);
                    }
                    let icon_task = self.load_file_icons(vec![icon_key], false);
                    let popup_task = self.open_download_detail_popup(task_id);
                    return Task::batch([icon_task, popup_task]);
                }
                Err(e) => {
                    self.toast = Some(format!("创建下载任务失败: {e}"));
                }
            },

            // Standalone Browser Resource Sniff & Selective Download Window
            Message::OpenEmptySniffPopup => {
                if let Some(existing) = &self.sniff_popup {
                    if let Some(hwnd) = existing.window_hwnd {
                        show_window_native(hwnd);
                    }
                    if let Some(wid) = existing.window_id {
                        self.active_input = InputTarget::SniffPageUrl;
                        return Task::batch([
                            window::gain_focus(wid),
                            operation::focus(views::sniff_popup::sniff_page_url_id()),
                        ]);
                    }
                }
                let task = self.open_or_update_sniff_popup(BrowserSniffPayload {
                    page_url: None,
                    page_title: None,
                    referrer: None,
                    cookie: None,
                    user_agent: None,
                    bring_to_front: true,
                    items: Vec::new(),
                });
                self.active_input = InputTarget::SniffPageUrl;
                return Task::batch([
                    task,
                    operation::focus(views::sniff_popup::sniff_page_url_id()),
                ]);
            }
            Message::SniffWindowOpened(id) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.window_id = Some(id);
                }
                return window::raw_id::<()>(id).map(Message::SniffWindowRawId);
            }
            Message::SniffWindowRawId(hwnd) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.window_hwnd = Some(hwnd);
                }
                self.apply_backdrop_now();
                show_window_native(hwnd);
                if let Some(shared) = SHARED.get() {
                    install_window_hooks(hwnd, shared.hook_tx.clone());
                }
            }
            Message::SniffDragWindow => {
                if let Some(wid) = self.sniff_popup.as_ref().and_then(|s| s.window_id) {
                    return window::drag(wid);
                }
            }
            Message::SniffClose => {
                self.context_menu = None;
                if let Some(s) = self.sniff_popup.take()
                    && let Some(wid) = s.window_id
                {
                    return window::close(wid);
                }
            }
            Message::SniffPageUrlChanged(v) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.page_url_input = v;
                    s.sniff_error = None;
                }
            }
            Message::SniffFetchPageUrl => {
                let Some(s) = &mut self.sniff_popup else {
                    return Task::none();
                };
                let url = s.page_url_input.trim().to_string();
                if url.is_empty() || s.sniffing_page {
                    return Task::none();
                }
                s.sniffing_page = true;
                s.sniff_error = None;
                let proxy = self.config.proxy_url.clone();
                let ua = self.config.user_agent.clone();
                return Task::perform(
                    async move {
                        sniff_webpage_url(&url, proxy.as_deref(), &ua)
                            .await
                            .map(|(title, items)| (url, title, items))
                    },
                    Message::SniffFetchPageFinished,
                );
            }
            Message::SniffFetchPageFinished(res) => {
                let mut should_auto_probe = false;
                if let Some(s) = &mut self.sniff_popup {
                    s.sniffing_page = false;
                    match res {
                        Ok((url, title, items)) => {
                            s.page_url = url.clone();
                            s.page_title = title;
                            s.sniff_error = None;
                            s.headers.insert("Referer".to_string(), url);
                            s.headers
                                .entry("User-Agent".to_string())
                                .or_insert_with(|| self.config.user_agent.clone());
                            s.filter = SniffFilterKind::All;
                            s.search.clear();
                            s.items.clear();

                            let has_non_image = items.iter().any(|i| i.kind != "image");
                            let mut unknown_http = 0usize;
                            for it in items {
                                let id = s.next_entry_id;
                                s.next_entry_id += 1;
                                let selected = if has_non_image {
                                    it.kind != "image"
                                } else {
                                    true
                                };
                                if it.file_size.is_none()
                                    && (it.url.starts_with("http://")
                                        || it.url.starts_with("https://"))
                                {
                                    unknown_http += 1;
                                }
                                s.items.push(SniffEntry {
                                    id,
                                    url: it.url,
                                    filename: it.filename,
                                    kind: it.kind,
                                    label: it.label,
                                    mime: it.mime,
                                    file_size: it.file_size,
                                    selected,
                                });
                            }
                            if s.items.is_empty() {
                                s.sniff_error = Some(
                                    "该页面未发现静态直链资源（如为动态 JS 播放页，请打开浏览器扩展开启嗅探或点击「扫描当前页」）".to_string(),
                                );
                            } else if unknown_http > 0 && unknown_http <= 20 {
                                should_auto_probe = true;
                            }
                        }
                        Err(e) => {
                            s.sniff_error = Some(e);
                        }
                    }
                }
                if should_auto_probe {
                    return self.update(Message::SniffProbeSizes);
                }
            }
            Message::SniffSetFilter(f) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.filter = f;
                }
            }
            Message::SniffSearchChanged(v) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.search = v;
                }
            }
            Message::SniffToggleItem(entry_id) => {
                if let Some(s) = &mut self.sniff_popup
                    && let Some(item) = s.items.iter_mut().find(|e| e.id == entry_id)
                {
                    item.selected = !item.selected;
                }
            }
            Message::SniffSelectAllVisible(select) => {
                if let Some(s) = &mut self.sniff_popup {
                    let visible_ids: HashSet<usize> = views::sniff_popup::visible_entries(s)
                        .into_iter()
                        .map(|e| e.id)
                        .collect();
                    for item in &mut s.items {
                        if visible_ids.contains(&item.id) {
                            item.selected = select;
                        }
                    }
                }
            }
            Message::SniffInvertSelection => {
                if let Some(s) = &mut self.sniff_popup {
                    let visible_ids: HashSet<usize> = views::sniff_popup::visible_entries(s)
                        .into_iter()
                        .map(|e| e.id)
                        .collect();
                    for item in &mut s.items {
                        if visible_ids.contains(&item.id) {
                            item.selected = !item.selected;
                        }
                    }
                }
            }
            Message::SniffProbeSizes => {
                let Some(s) = &mut self.sniff_popup else {
                    return Task::none();
                };
                if s.probing_sizes {
                    return Task::none();
                }
                let urls: Vec<String> = views::sniff_popup::visible_entries(s)
                    .into_iter()
                    .filter(|e| {
                        e.file_size.is_none()
                            && (e.url.starts_with("http://") || e.url.starts_with("https://"))
                    })
                    .take(24)
                    .map(|e| e.url.clone())
                    .collect();
                if urls.is_empty() {
                    return Task::none();
                }
                s.probing_sizes = true;
                let headers = s.headers.clone();
                let proxy = self.config.proxy_url.clone();
                let ua = self.config.user_agent.clone();
                return Task::perform(
                    async move { probe_urls_sizes(urls, headers, proxy, ua).await },
                    Message::SniffProbeSizesFinished,
                );
            }
            Message::SniffProbeSizesFinished(results) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.probing_sizes = false;
                    for (url, size) in results {
                        if let Some(sz) = size
                            && let Some(item) = s.items.iter_mut().find(|e| e.url == url)
                        {
                            item.file_size = Some(sz);
                        }
                    }
                }
            }
            Message::SniffRemoveItem(entry_id) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.items.retain(|e| e.id != entry_id);
                }
            }
            Message::SniffClearAll => {
                if let Some(s) = &mut self.sniff_popup {
                    s.items.clear();
                    s.sniff_error = None;
                }
            }
            Message::SniffSaveDirChanged(v) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.save_dir = v;
                }
            }
            Message::SniffConnectionsChanged(v) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.connections = (v.round() as u32).clamp(1, 32);
                }
            }
            Message::SniffBrowseDir => {
                let start = self
                    .sniff_popup
                    .as_ref()
                    .map(|s| PathBuf::from(&s.save_dir))
                    .unwrap_or_else(|| self.config.default_save_dir.clone());
                return Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new();
                        if start.is_dir() {
                            dlg = dlg.set_directory(start);
                        }
                        dlg.pick_folder().await.map(|h| h.path().to_path_buf())
                    },
                    Message::SniffDirPicked,
                );
            }
            Message::SniffDirPicked(Some(dir)) => {
                if let Some(s) = &mut self.sniff_popup {
                    s.save_dir = dir.to_string_lossy().to_string();
                }
            }
            Message::SniffDirPicked(None) => {}
            Message::SniffDownloadSingle(entry_id) => {
                if let Some(s) = &mut self.sniff_popup
                    && let Some(idx) = s.items.iter().position(|e| e.id == entry_id)
                {
                    let entry = s.items.remove(idx);
                    let headers = s.headers.clone();
                    let connections = s.connections.clamp(1, 32);
                    let save_dir = {
                        let d = s.save_dir.trim();
                        if d.is_empty() {
                            None
                        } else {
                            Some(PathBuf::from(d))
                        }
                    };
                    let req = NewTaskRequest {
                        url: entry.url,
                        filename: Some(entry.filename.clone()),
                        save_dir,
                        max_connections: Some(connections),
                        speed_limit_bps: None,
                        headers,
                        checksum: None,
                        source: TaskSource::Browser,
                        auto_start: true,
                    };
                    self.toast = Some(format!("已从嗅探选择窗开始下载: {}", entry.filename));
                    let mgr = Arc::clone(&self.manager);
                    return Task::perform(
                        async move { mgr.add_task(req).await.map_err(|e| e.to_string()) },
                        Message::CaptureStartedWithDialog,
                    );
                }
            }
            Message::SniffDownloadSelected(auto_start) => {
                if let Some(s) = &mut self.sniff_popup {
                    let chosen: Vec<SniffEntry> =
                        s.items.iter().filter(|e| e.selected).cloned().collect();
                    if chosen.is_empty() {
                        return Task::none();
                    }
                    let count = chosen.len();
                    let headers = s.headers.clone();
                    let connections = s.connections.clamp(1, 32);
                    let save_dir = {
                        let d = s.save_dir.trim();
                        if d.is_empty() {
                            None
                        } else {
                            Some(PathBuf::from(d))
                        }
                    };
                    s.items.retain(|e| !e.selected);
                    let close_win = if s.items.is_empty() {
                        self.sniff_popup.take().and_then(|st| st.window_id)
                    } else {
                        None
                    };
                    self.toast = Some(format!(
                        "已从嗅探选择窗提交 {count} 个任务（{}）",
                        if auto_start { "开始下载" } else { "加入暂停队列" }
                    ));
                    let mgr = Arc::clone(&self.manager);
                    let submit_task = Task::perform(
                        async move {
                            let mut last_err = None;
                            for entry in chosen {
                                let req = NewTaskRequest {
                                    url: entry.url,
                                    filename: Some(entry.filename),
                                    save_dir: save_dir.clone(),
                                    max_connections: Some(connections),
                                    speed_limit_bps: None,
                                    headers: headers.clone(),
                                    checksum: None,
                                    source: TaskSource::Browser,
                                    auto_start,
                                };
                                if let Err(e) = mgr.add_task(req).await {
                                    last_err = Some(e.to_string());
                                }
                            }
                            match last_err {
                                Some(e) => Err(e),
                                None => Ok(()),
                            }
                        },
                        Message::TaskOpFinished,
                    );
                    if let Some(wid) = close_win {
                        return Task::batch([window::close(wid), submit_task]);
                    }
                    return submit_task;
                }
            }
            Message::OpenTaskDetailPopup(task_id) => {
                return self.open_download_detail_popup(task_id);
            }
            Message::DownloadPopupRawId(win_id, hwnd) => {
                if let Some(dp) = self.download_popups.get_mut(&win_id) {
                    dp.window_hwnd = Some(hwnd);
                }
                self.apply_backdrop_now();
                if let Some(shared) = SHARED.get() {
                    install_window_hooks(hwnd, shared.hook_tx.clone());
                }
            }
            Message::DownloadPopupDrag(win_id) => {
                return window::drag(win_id);
            }
            Message::DownloadPopupToggleAutoClose(win_id, v) => {
                if let Some(dp) = self.download_popups.get_mut(&win_id) {
                    dp.auto_close_on_complete = v;
                }
            }
            Message::DownloadPopupClose(win_id) => {
                self.download_popups.remove(&win_id);
                return window::close(win_id);
            }
            Message::DownloadPopupCancelTask(win_id, task_id) => {
                self.download_popups.remove(&win_id);
                let mgr = Arc::clone(&self.manager);
                return Task::batch([
                    window::close(win_id),
                    Task::perform(
                        async move { mgr.remove_task(task_id, true).await.map_err(|e| e.to_string()) },
                        Message::TaskOpFinished,
                    ),
                ]);
            }
            Message::DownloadPopupOpenFile(win_id, task_id) => {
                self.download_popups.remove(&win_id);
                if let Some(t) = self.tasks.iter().find(|t| t.id == task_id) {
                    open_file(&t.target_path());
                }
                return window::close(win_id);
            }
            Message::DismissClipboardBanner => {
                self.clipboard_banner_url = None;
            }
            Message::ClipboardQuickDownload => {
                if let Some(url) = self.clipboard_banner_url.take() {
                    let mgr = Arc::clone(&self.manager);
                    let req = NewTaskRequest::quick(url, TaskSource::Clipboard);
                    return Task::perform(
                        async move { mgr.add_task(req).await.map(|_| ()).map_err(|e| e.to_string()) },
                        Message::TaskOpFinished,
                    );
                }
            }
            Message::ClipboardOpenModal => {
                let url = self.clipboard_banner_url.take();
                return self.update(Message::OpenNewTaskModal(url));
            }

            // Navigation & quickbar
            Message::SelectNav(sec) => {
                self.active_nav = sec;
            }
            Message::QuickBarChanged(val) => {
                self.quick_input = val;
            }
            Message::QuickBarSubmit => {
                let raw = self.quick_input.trim().to_string();
                let low = raw.to_ascii_lowercase();
                if low.starts_with("http://")
                    || low.starts_with("https://")
                    || low.starts_with("magnet:?")
                    || low.starts_with("ed2k://|file|")
                {
                    self.quick_input.clear();
                    self.active_nav = NavSection::All;
                    let mgr = Arc::clone(&self.manager);
                    let req = NewTaskRequest::quick(raw, TaskSource::QuickBar);
                    return Task::perform(
                        async move { mgr.add_task(req).await.map(|_| ()).map_err(|e| e.to_string()) },
                        Message::TaskOpFinished,
                    );
                }
            }
            Message::CycleTheme => {
                self.config.theme_mode = match self.config.theme_mode {
                    ThemeMode::Dark => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Dark,
                    ThemeMode::System => {
                        if self.system_dark {
                            ThemeMode::Light
                        } else {
                            ThemeMode::Dark
                        }
                    }
                };
                self.apply_backdrop_now();
                return self.save_config_async();
            }

            // Task controls
            Message::SelectTask(id) => {
                self.selected_task_id = Some(id);
                self.show_replace_url = false;
                self.replace_url_input.clear();
            }
            Message::CloseInspector => {
                self.selected_task_id = None;
            }
            Message::PauseTask(id) => {
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move { mgr.pause_task(id).await.map_err(|e| e.to_string()) },
                    Message::TaskOpFinished,
                );
            }
            Message::ResumeTask(id) => {
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move { mgr.start_task(id).await.map_err(|e| e.to_string()) },
                    Message::TaskOpFinished,
                );
            }
            Message::PauseAll => {
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        mgr.pause_all().await;
                        Ok(())
                    },
                    Message::TaskOpFinished,
                );
            }
            Message::ResumeAll => {
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        mgr.resume_all().await;
                        Ok(())
                    },
                    Message::TaskOpFinished,
                );
            }
            Message::RequestDeleteTask(id) => {
                if self.config.confirm_on_delete {
                    if let Some(t) = self.tasks.iter().find(|t| t.id == id) {
                        let file_path = if t.status == TaskStatus::Completed {
                            t.target_path()
                        } else if t.part_path().exists() {
                            t.part_path()
                        } else {
                            t.target_path()
                        };
                        self.context_menu = None;
                        self.delete_confirm = Some(DeleteConfirmState {
                            task_id: id,
                            filename: t.filename.clone(),
                            file_path,
                            delete_files: self.config.delete_files_by_default,
                            dont_ask_again: false,
                        });
                    }
                } else {
                    let delete_files = self.config.delete_files_by_default;
                    return self.update(Message::RemoveTask(id, delete_files));
                }
            }
            Message::DeleteConfirmToggleFiles(v) => {
                if let Some(dlg) = &mut self.delete_confirm {
                    dlg.delete_files = v;
                }
            }
            Message::DeleteConfirmToggleDontAsk(v) => {
                if let Some(dlg) = &mut self.delete_confirm {
                    dlg.dont_ask_again = v;
                }
            }
            Message::DeleteConfirmCancel => {
                self.delete_confirm = None;
            }
            Message::DeleteConfirmAccept => {
                if let Some(dlg) = self.delete_confirm.take() {
                    let mut batch = Vec::new();
                    if dlg.dont_ask_again {
                        self.config.confirm_on_delete = false;
                        self.config.delete_files_by_default = dlg.delete_files;
                        batch.push(self.save_config_async());
                    }
                    batch.push(self.update(Message::RemoveTask(dlg.task_id, dlg.delete_files)));
                    return Task::batch(batch);
                }
            }
            Message::RemoveTask(id, delete_files) => {
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        mgr.remove_task(id, delete_files)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::TaskOpFinished,
                );
            }
            Message::OpenTaskFile(id) => {
                if let Some(t) = self.tasks.iter().find(|t| t.id == id) {
                    open_file(&t.target_path());
                }
            }
            Message::RevealTaskInExplorer(id) => {
                if let Some(t) = self.tasks.iter().find(|t| t.id == id) {
                    let target = if t.target_path().exists() {
                        t.target_path()
                    } else {
                        t.part_path()
                    };
                    reveal_in_explorer(&target);
                }
            }
            Message::AdjustTaskConnections(id, delta) => {
                if let Some(t) = self.tasks.iter().find(|t| t.id == id) {
                    let next = (t.max_connections as i32 + delta).clamp(1, 32) as u32;
                    let mgr = Arc::clone(&self.manager);
                    return Task::perform(
                        async move {
                            mgr.update_task_connections(id, next)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::TaskOpFinished,
                    );
                }
            }
            Message::SetTaskSpeedLimit(id, bps) => {
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        mgr.update_task_speed_limit(id, bps)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::TaskOpFinished,
                );
            }
            Message::ToggleReplaceUrlBox => {
                self.show_replace_url = !self.show_replace_url;
                if self.show_replace_url {
                    self.active_input = InputTarget::ReplaceUrl;
                    return operation::focus(views::inspector::replace_url_id());
                }
            }
            Message::ReplaceUrlInputChanged(v) => {
                self.replace_url_input = v;
            }
            Message::SubmitReplaceUrl(id) => {
                let new_url = self.replace_url_input.trim().to_string();
                if !new_url.is_empty() {
                    self.show_replace_url = false;
                    self.replace_url_input.clear();
                    let mgr = Arc::clone(&self.manager);
                    return Task::perform(
                        async move {
                            mgr.replace_task_url(id, new_url, true)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::TaskOpFinished,
                    );
                }
            }
            Message::TaskOpFinished(Err(e)) => {
                self.toast = Some(e);
            }
            Message::TaskOpFinished(Ok(())) => {}

            // New Task modal
            Message::OpenNewTaskModal(prefill_url) => {
                self.modal = NewTaskModalState {
                    open: true,
                    batch_mode: false,
                    url: prefill_url.clone().unwrap_or_default(),
                    batch_text: String::new(),
                    batch_filter: String::new(),
                    filename: String::new(),
                    save_dir: self.config.default_save_dir.to_string_lossy().to_string(),
                    connections: self.config.default_connections,
                    speed_limit_kbps: "0".to_string(),
                    sha256: String::new(),
                    probing: false,
                    probe_error: None,
                    preview: None,
                };
                self.active_input = InputTarget::ModalUrl;
                self.context_menu = None;
                let focus_task = operation::focus(views::new_task::modal_url_id());
                if prefill_url.is_some() {
                    let probe = self.update(Message::ModalProbeUrl);
                    return Task::batch([focus_task, probe]);
                }
                return focus_task;
            }
            Message::CloseNewTaskModal => {
                self.modal.open = false;
                self.active_input = InputTarget::QuickBar;
            }
            Message::ModalUrlChanged(v) => {
                self.modal.url = v;
                self.modal.probe_error = None;
            }
            Message::ModalFilenameChanged(v) => self.modal.filename = v,
            Message::ModalSaveDirChanged(v) => self.modal.save_dir = v,
            Message::ModalBrowseDir => {
                let start = PathBuf::from(&self.modal.save_dir);
                return Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new();
                        if start.is_dir() {
                            dlg = dlg.set_directory(start);
                        }
                        dlg.pick_folder().await.map(|h| h.path().to_path_buf())
                    },
                    Message::ModalDirPicked,
                );
            }
            Message::ModalDirPicked(Some(dir)) => {
                self.modal.save_dir = dir.to_string_lossy().to_string();
            }
            Message::ModalDirPicked(None) => {}
            Message::ModalConnectionsChanged(v) => {
                self.modal.connections = (v.round() as u32).clamp(1, 32);
            }
            Message::ModalSpeedLimitChanged(v) => self.modal.speed_limit_kbps = v,
            Message::ModalSha256Changed(v) => self.modal.sha256 = v,
            Message::ModalProbeUrl => {
                let url = self.modal.url.trim().to_string();
                if url.is_empty() {
                    return Task::none();
                }
                self.modal.probing = true;
                self.modal.probe_error = None;
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        mgr.probe_url(&url, &BTreeMap::new(), TaskSource::Manual)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::ModalProbeFinished,
                );
            }
            Message::ModalProbeFinished(res) => {
                self.modal.probing = false;
                match res {
                    Ok(preview) => {
                        self.modal.filename = preview.metadata.filename.clone();
                        self.modal.save_dir =
                            preview.suggested_save_dir.to_string_lossy().to_string();
                        self.modal.connections = preview.suggested_connections;
                        if preview.suggested_speed_limit_bps > 0 {
                            self.modal.speed_limit_kbps =
                                (preview.suggested_speed_limit_bps / 1024).to_string();
                        }
                        self.modal.preview = Some(preview);
                    }
                    Err(e) => {
                        self.modal.probe_error = Some(e);
                    }
                }
            }
            Message::ModalSubmit(auto_start) => {
                let url = self.modal.url.trim().to_string();
                if url.is_empty() {
                    return Task::none();
                }
                let filename = {
                    let f = self.modal.filename.trim();
                    if f.is_empty() {
                        None
                    } else {
                        Some(f.to_string())
                    }
                };
                let save_dir = {
                    let d = self.modal.save_dir.trim();
                    if d.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(d))
                    }
                };
                let speed_limit_bps = self
                    .modal
                    .speed_limit_kbps
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(0)
                    .saturating_mul(1024);
                let checksum = {
                    let h = self.modal.sha256.trim();
                    if h.is_empty() {
                        None
                    } else {
                        Some(ChecksumSpec {
                            algorithm: HashAlgorithm::Sha256,
                            expected: h.to_string(),
                        })
                    }
                };
                let headers = self
                    .modal
                    .preview
                    .as_ref()
                    .map(|p| p.metadata.resolved_headers.clone())
                    .unwrap_or_default();

                let req = NewTaskRequest {
                    url,
                    filename,
                    save_dir,
                    max_connections: Some(self.modal.connections),
                    speed_limit_bps: Some(speed_limit_bps),
                    headers,
                    checksum,
                    source: TaskSource::Manual,
                    auto_start,
                };

                self.modal.open = false;
                self.active_nav = NavSection::All;
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move { mgr.add_task(req).await.map(|_| ()).map_err(|e| e.to_string()) },
                    Message::TaskOpFinished,
                );
            }
            Message::ModalToggleBatchMode(batch) => {
                self.modal.batch_mode = batch;
                if batch {
                    self.active_input = InputTarget::ModalBatchUrls;
                    return operation::focus(views::new_task::modal_batch_id());
                } else {
                    self.active_input = InputTarget::ModalUrl;
                    return operation::focus(views::new_task::modal_url_id());
                }
            }
            Message::ModalBatchTextChanged(v) => {
                self.modal.batch_text = v;
            }
            Message::ModalBatchFilterChanged(v) => {
                self.modal.batch_filter = v;
            }
            Message::ModalBatchPasteClipboard => {
                if let Some(raw) = read_clipboard_text() {
                    let trimmed = raw.trim();
                    if !trimmed.is_empty() {
                        if self.modal.batch_text.trim().is_empty() {
                            self.modal.batch_text = trimmed.to_string();
                        } else {
                            self.modal.batch_text.push(' ');
                            self.modal.batch_text.push_str(trimmed);
                        }
                    }
                }
            }
            Message::ModalPickTorrentFile => {
                let start = PathBuf::from(&self.modal.save_dir);
                return Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .set_title("选择 BitTorrent 种子文件 (.torrent)")
                            .add_filter("BitTorrent 种子文件", &["torrent"]);
                        if start.is_dir() {
                            dlg = dlg.set_directory(start);
                        }
                        dlg.pick_file().await.map(|h| h.path().to_path_buf())
                    },
                    Message::ModalTorrentFilePicked,
                );
            }
            Message::ModalTorrentFilePicked(Some(path)) => {
                if let Ok(file_url) = url::Url::from_file_path(&path) {
                    self.modal.url = file_url.to_string();
                    self.modal.probe_error = None;
                    return self.update(Message::ModalProbeUrl);
                }
            }
            Message::ModalTorrentFilePicked(None) => {}
            Message::ModalBatchSubmit(auto_start) => {
                let expanded = expand_batch_input(&self.modal.batch_text, &self.modal.batch_filter);
                if expanded.urls.is_empty() {
                    return Task::none();
                }
                let count = expanded.urls.len();
                let save_dir = {
                    let d = self.modal.save_dir.trim();
                    if d.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(d))
                    }
                };
                let connections = self.modal.connections;
                let speed_limit_bps = self
                    .modal
                    .speed_limit_kbps
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(0)
                    .saturating_mul(1024);

                self.modal.open = false;
                self.active_nav = NavSection::All;
                self.toast = Some(format!(
                    "已批量导入 {count} 个任务（{}）",
                    if auto_start { "自动排队下载" } else { "暂停队列" }
                ));
                let mgr = Arc::clone(&self.manager);
                let urls = expanded.urls;
                return Task::perform(
                    async move {
                        let mut last_err = None;
                        for url in urls {
                            let req = NewTaskRequest {
                                url,
                                filename: None,
                                save_dir: save_dir.clone(),
                                max_connections: Some(connections),
                                speed_limit_bps: Some(speed_limit_bps),
                                headers: BTreeMap::new(),
                                checksum: None,
                                source: TaskSource::Manual,
                                auto_start,
                            };
                            if let Err(e) = mgr.add_task(req).await {
                                last_err = Some(e.to_string());
                            }
                        }
                        match last_err {
                            Some(e) => Err(e),
                            None => Ok(()),
                        }
                    },
                    Message::TaskOpFinished,
                );
            }

            // Scheduler & Queue Complete Actions
            Message::SchedToggleStart(v) => {
                self.config.schedule.start_enabled = v;
                return self.save_config_async();
            }
            Message::SchedStartTimeChanged(v) => {
                self.config.schedule.start_time = v;
            }
            Message::SchedToggleStop(v) => {
                self.config.schedule.stop_enabled = v;
                return self.save_config_async();
            }
            Message::SchedStopTimeChanged(v) => {
                self.config.schedule.stop_time = v;
            }
            Message::SchedToggleSpeed(v) => {
                self.config.schedule.speed_schedule_enabled = v;
                let save = self.save_config_async();
                let tick = self.update(Message::SchedulerTick);
                return Task::batch([save, tick]);
            }
            Message::SchedSpeedStartChanged(v) => {
                self.config.schedule.speed_start_time = v;
            }
            Message::SchedSpeedEndChanged(v) => {
                self.config.schedule.speed_end_time = v;
            }
            Message::SchedSpeedKbpsChanged(v) => {
                self.sched_speed_kbps_input = v.clone();
                self.config.schedule.scheduled_speed_limit_kbps =
                    v.trim().parse::<u64>().unwrap_or(0);
            }
            Message::SchedSetCompleteAction(act) => {
                self.config.schedule.on_queue_complete = act;
                return self.save_config_async();
            }
            Message::SchedScriptChanged(v) => {
                self.config.schedule.post_queue_script = v;
            }
            Message::SchedToggleOneShot(v) => {
                self.config.schedule.one_shot_complete_action = v;
                return self.save_config_async();
            }
            Message::SchedSave => {
                self.config.schedule.scheduled_speed_limit_kbps = self
                    .sched_speed_kbps_input
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(0);
                self.toast = Some("计划调度与完成动作已保存".to_string());
                let save = self.save_config_async();
                let tick = self.update(Message::SchedulerTick);
                return Task::batch([save, tick]);
            }
            Message::SchedulerTick => {
                use chrono::{Datelike, Timelike};
                let now = chrono::Local::now();
                let today = (now.year(), now.month(), now.day());
                let now_mins = (now.hour() * 60 + now.minute()) as u16;
                let mut tasks_to_run: Vec<Task<Message>> = Vec::new();

                if self.config.schedule.start_enabled
                    && parse_hhmm(&self.config.schedule.start_time) == Some(now_mins)
                    && self.sched_last_start_day != Some(today)
                {
                    self.sched_last_start_day = Some(today);
                    self.toast = Some(format!(
                        "定时任务触发（{}）：已自动开始下载队列",
                        self.config.schedule.start_time
                    ));
                    tasks_to_run.push(self.update(Message::ResumeAll));
                }

                if self.config.schedule.stop_enabled
                    && parse_hhmm(&self.config.schedule.stop_time) == Some(now_mins)
                    && self.sched_last_stop_day != Some(today)
                {
                    self.sched_last_stop_day = Some(today);
                    self.toast = Some(format!(
                        "定时任务触发（{}）：已自动暂停全部任务",
                        self.config.schedule.stop_time
                    ));
                    tasks_to_run.push(self.update(Message::PauseAll));
                }

                let should_cap = self.config.schedule.speed_schedule_enabled
                    && is_time_in_window(
                        now_mins,
                        &self.config.schedule.speed_start_time,
                        &self.config.schedule.speed_end_time,
                    );

                if should_cap && !self.sched_speed_window_active {
                    self.sched_speed_window_active = true;
                    let bps = self
                        .config
                        .schedule
                        .scheduled_speed_limit_kbps
                        .saturating_mul(1024);
                    let mgr = Arc::clone(&self.manager);
                    tasks_to_run.push(Task::perform(
                        async move {
                            mgr.update_global_speed_limit(bps)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::TaskOpFinished,
                    ));
                } else if !should_cap && self.sched_speed_window_active {
                    self.sched_speed_window_active = false;
                    let normal_bps = self.config.global_speed_limit_bps;
                    let mgr = Arc::clone(&self.manager);
                    tasks_to_run.push(Task::perform(
                        async move {
                            mgr.update_global_speed_limit(normal_bps)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::TaskOpFinished,
                    ));
                }

                if !tasks_to_run.is_empty() {
                    return Task::batch(tasks_to_run);
                }
            }

            // BitTorrent & Custom Tracker dedicated workspace
            Message::BtToggleDht(v) => {
                self.config.bt.enable_dht = v;
                return self.save_config_async();
            }
            Message::BtToggleLsd(v) => {
                self.config.bt.enable_lsd = v;
                return self.save_config_async();
            }
            Message::BtToggleUpnp(v) => {
                self.config.bt.enable_upnp = v;
                return self.save_config_async();
            }
            Message::BtToggleDisableTrackers(v) => {
                self.config.bt.disable_trackers = v;
                return self.save_config_async();
            }
            Message::BtToggleAutoAppend(v) => {
                self.config.bt.auto_append_trackers = v;
                return self.save_config_async();
            }
            Message::BtToggleAutoSyncOnStart(v) => {
                self.config.bt.auto_sync_trackers_on_start = v;
                return self.save_config_async();
            }
            Message::BtListenPortChanged(v) => {
                self.bt_listen_port_input = v;
            }
            Message::BtMaxPeersChanged(v) => {
                self.bt_max_peers_input = v;
            }
            Message::BtConnectTimeoutChanged(v) => {
                self.bt_timeout_input = v;
            }
            Message::BtNewTrackerChanged(v) => {
                self.bt_new_tracker_input = v;
            }
            Message::BtAddTracker => {
                let parsed = parse_tracker_list(&self.bt_new_tracker_input);
                if parsed.is_empty() {
                    self.toast = Some(
                        "请输入有效的 Tracker 地址（支持 udp://、http://、https://、ws://、wss://）"
                            .to_string(),
                    );
                    return Task::none();
                }
                let mut added = 0usize;
                for tr in parsed {
                    if !self.config.bt.custom_trackers.contains(&tr) {
                        self.config.bt.custom_trackers.push(tr);
                        added += 1;
                    }
                }
                self.bt_new_tracker_input.clear();
                self.toast = Some(if added > 0 {
                    format!("已添加 {added} 条自定义 Tracker")
                } else {
                    "输入的 Tracker 已存在于列表中".to_string()
                });
                return self.save_config_async();
            }
            Message::BtPasteTrackersClipboard => {
                let Some(raw) = read_clipboard_text() else {
                    self.toast = Some("剪贴板为空".to_string());
                    return Task::none();
                };
                let parsed = parse_tracker_list(&raw);
                if parsed.is_empty() {
                    self.toast = Some("未在剪贴板内容中检测到有效的 Tracker 地址".to_string());
                    return Task::none();
                }
                let mut added = 0usize;
                for tr in parsed {
                    if !self.config.bt.custom_trackers.contains(&tr) {
                        self.config.bt.custom_trackers.push(tr);
                        added += 1;
                    }
                }
                self.toast = Some(format!("已从剪贴板导入 {added} 条自定义 Tracker"));
                return self.save_config_async();
            }
            Message::BtRemoveCustomTracker(idx) => {
                if idx < self.config.bt.custom_trackers.len() {
                    self.config.bt.custom_trackers.remove(idx);
                    return self.save_config_async();
                }
            }
            Message::BtClearCustomTrackers => {
                self.config.bt.custom_trackers.clear();
                self.toast = Some("已清空自定义 Tracker 列表".to_string());
                return self.save_config_async();
            }
            Message::BtNewSubscribeUrlChanged(v) => {
                self.bt_new_sub_url_input = v;
            }
            Message::BtAddSubscribeUrl => {
                let u = self.bt_new_sub_url_input.trim().to_string();
                if !(u.starts_with("http://") || u.starts_with("https://")) {
                    self.toast = Some("订阅源地址必须以 http:// 或 https:// 开头".to_string());
                    return Task::none();
                }
                if !self.config.bt.tracker_subscribe_urls.contains(&u) {
                    self.config.bt.tracker_subscribe_urls.push(u);
                }
                self.bt_new_sub_url_input.clear();
                self.toast = Some("已添加公共 Tracker 订阅源".to_string());
                return self.save_config_async();
            }
            Message::BtRemoveSubscribeUrl(idx) => {
                if idx < self.config.bt.tracker_subscribe_urls.len() {
                    self.config.bt.tracker_subscribe_urls.remove(idx);
                    return self.save_config_async();
                }
            }
            Message::BtResetDefaultSubscribeUrls => {
                self.config.bt.tracker_subscribe_urls = default_tracker_subscribe_urls();
                self.toast = Some("已恢复默认公共 Tracker 订阅源".to_string());
                return self.save_config_async();
            }
            Message::BtSyncTrackers => {
                if self.bt_syncing_trackers {
                    return Task::none();
                }
                self.bt_syncing_trackers = true;
                self.bt_sync_status = Some("正在拉取并去重公共 Tracker 订阅源...".to_string());
                let urls = self.config.bt.tracker_subscribe_urls.clone();
                let proxy = self.config.proxy_url.clone();
                return Task::perform(
                    async move {
                        sync_trackers_from_urls(&urls, proxy.as_deref())
                            .await
                            .map(|r| (r.trackers, r.sources_succeeded, r.sources_failed))
                    },
                    Message::BtSyncTrackersFinished,
                );
            }
            Message::BtSyncTrackersFinished(res) => {
                self.bt_syncing_trackers = false;
                match res {
                    Ok((trackers, ok_cnt, fail_cnt)) => {
                        let count = trackers.len();
                        let now_ts = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
                        self.config.bt.synced_trackers = trackers;
                        self.config.bt.last_tracker_sync_at = Some(now_ts);
                        let msg = if fail_cnt > 0 {
                            format!(
                                "同步完成：获取 {count} 条去重 Tracker（{ok_cnt} 个源成功，{fail_cnt} 个源失败）"
                            )
                        } else {
                            format!("同步完成：成功从 {ok_cnt} 个订阅源获取 {count} 条活跃 Tracker")
                        };
                        self.bt_sync_status = Some(msg.clone());
                        self.toast = Some(msg);
                        return self.save_config_async();
                    }
                    Err(e) => {
                        self.bt_sync_status = Some(format!("同步失败：{e}"));
                        self.toast = Some(format!("同步 Tracker 失败：{e}"));
                    }
                }
            }
            Message::BtClearSyncedTrackers => {
                self.config.bt.synced_trackers.clear();
                self.config.bt.last_tracker_sync_at = None;
                self.bt_sync_status = Some("已清空订阅同步缓存".to_string());
                return self.save_config_async();
            }
            Message::BtSave => {
                self.config.bt.listen_port = self
                    .bt_listen_port_input
                    .trim()
                    .parse::<u16>()
                    .unwrap_or(0);
                self.config.bt.max_peers_per_torrent = self
                    .bt_max_peers_input
                    .trim()
                    .parse::<usize>()
                    .unwrap_or(128)
                    .clamp(10, 500);
                self.config.bt.peer_connect_timeout_secs = self
                    .bt_timeout_input
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(10)
                    .clamp(3, 60);
                self.bt_listen_port_input = self.config.bt.listen_port.to_string();
                self.bt_max_peers_input = self.config.bt.max_peers_per_torrent.to_string();
                self.bt_timeout_input = self.config.bt.peer_connect_timeout_secs.to_string();
                self.toast = Some("BitTorrent 与自定义 Tracker 设置已保存".to_string());
                if let Some(shared) = SHARED.get() {
                    let bt_engine = Arc::clone(&shared.bt_engine);
                    let bt_cfg = self.config.bt.clone();
                    let out_dir = self.config.default_save_dir.clone();
                    let proxy = self.config.proxy_url.clone();
                    let save_task = self.save_config_async();
                    let warm_task = Task::perform(
                        async move {
                            bt_engine.warmup(&bt_cfg, &out_dir, proxy.as_deref()).await;
                        },
                        |_| Message::ClearToast,
                    );
                    return Task::batch([save_task, warm_task]);
                }
                return self.save_config_async();
            }

            // eD2k dedicated workspace
            Message::Ed2kToggleEnabled(v) => {
                self.config.ed2k.enabled = v;
                return self.save_config_async();
            }
            Message::Ed2kNicknameChanged(v) => {
                self.config.ed2k.nickname = v;
            }
            Message::Ed2kTcpPortChanged(v) => {
                self.ed2k_tcp_port_input = v;
            }
            Message::Ed2kUdpPortChanged(v) => {
                self.ed2k_udp_port_input = v;
            }
            Message::Ed2kToggleKad(v) => {
                self.config.ed2k.enable_kad = v;
                return self.save_config_async();
            }
            Message::Ed2kToggleObfuscation(v) => {
                self.config.ed2k.enable_obfuscation = v;
                return self.save_config_async();
            }
            Message::Ed2kMaxSourcesChanged(v) => {
                self.ed2k_max_sources_input = v;
            }
            Message::Ed2kUploadLimitChanged(v) => {
                self.ed2k_upload_limit_input = v;
            }
            Message::Ed2kToggleVerifyMd4(v) => {
                self.config.ed2k.verify_md4_hash = v;
                return self.save_config_async();
            }
            Message::Ed2kServerMetUrlChanged(v) => {
                self.config.ed2k.server_met_url = v;
            }
            Message::Ed2kSyncServerMet => {
                if self.ed2k_syncing_met {
                    return Task::none();
                }
                self.ed2k_syncing_met = true;
                self.ed2k_met_sync_status = Some("正在拉取并解析 server.met 服务器列表...".to_string());
                let url = self.config.ed2k.server_met_url.clone();
                let proxy = self.config.proxy_url.clone();
                return Task::perform(
                    async move { sync_servers_from_met(&url, proxy.as_deref()).await },
                    Message::Ed2kSyncServerMetFinished,
                );
            }
            Message::Ed2kSyncServerMetFinished(res) => {
                self.ed2k_syncing_met = false;
                match res {
                    Ok(list) => {
                        let mut added = 0usize;
                        for entry in list {
                            if !self
                                .config
                                .ed2k
                                .servers
                                .iter()
                                .any(|s| s.host == entry.host && s.port == entry.port)
                            {
                                self.config.ed2k.servers.push(entry);
                                added += 1;
                            }
                        }
                        let msg = format!(
                            "server.met 同步成功：新增 {added} 台服务器，当前共 {} 台",
                            self.config.ed2k.servers.len()
                        );
                        self.ed2k_met_sync_status = Some(msg.clone());
                        self.toast = Some(msg);
                        return self.save_config_async();
                    }
                    Err(e) => {
                        self.ed2k_met_sync_status = Some(format!("同步 server.met 失败：{e}"));
                        self.toast = Some(format!("同步 server.met 失败：{e}"));
                    }
                }
            }
            Message::Ed2kQuickServerLinkChanged(v) => {
                self.ed2k_quick_server_link = v;
            }
            Message::Ed2kImportQuickServerLink => {
                if let Some(entry) = parse_ed2k_server_link(&self.ed2k_quick_server_link) {
                    if !self
                        .config
                        .ed2k
                        .servers
                        .iter()
                        .any(|s| s.host == entry.host && s.port == entry.port)
                    {
                        self.config.ed2k.servers.push(entry.clone());
                        self.toast = Some(format!("已导入 eD2k 服务器 {}:{}", entry.host, entry.port));
                    } else {
                        self.toast = Some("该 eD2k 服务器已在列表中".to_string());
                    }
                    self.ed2k_quick_server_link.clear();
                    return self.save_config_async();
                } else {
                    self.toast = Some(
                        "格式无效，请输入 ed2k://|server|IP|PORT|/ 或 IP:PORT".to_string(),
                    );
                }
            }
            Message::Ed2kNewServerNameChanged(v) => {
                self.ed2k_new_srv_name = v;
            }
            Message::Ed2kNewServerHostChanged(v) => {
                self.ed2k_new_srv_host = v;
            }
            Message::Ed2kNewServerPortChanged(v) => {
                self.ed2k_new_srv_port = v;
            }
            Message::Ed2kAddServer => {
                let host = self.ed2k_new_srv_host.trim().to_string();
                let Ok(port) = self.ed2k_new_srv_port.trim().parse::<u16>() else {
                    self.toast = Some("请输入有效的端口号（1–65535）".to_string());
                    return Task::none();
                };
                if host.is_empty() || port == 0 {
                    self.toast = Some("请输入有效的服务器主机与端口".to_string());
                    return Task::none();
                }
                let name = if self.ed2k_new_srv_name.trim().is_empty() {
                    format!("eD2k {host}:{port}")
                } else {
                    self.ed2k_new_srv_name.trim().to_string()
                };
                if !self
                    .config
                    .ed2k
                    .servers
                    .iter()
                    .any(|s| s.host == host && s.port == port)
                {
                    self.config.ed2k.servers.push(Ed2kServerEntry {
                        name,
                        host: host.clone(),
                        port,
                        enabled: true,
                    });
                    self.toast = Some(format!("已添加 eD2k 服务器 {host}:{port}"));
                } else {
                    self.toast = Some("该 eD2k 服务器已存在".to_string());
                }
                self.ed2k_new_srv_name.clear();
                self.ed2k_new_srv_host.clear();
                return self.save_config_async();
            }
            Message::Ed2kToggleServerEnabled(idx) => {
                if let Some(srv) = self.config.ed2k.servers.get_mut(idx) {
                    srv.enabled = !srv.enabled;
                    return self.save_config_async();
                }
            }
            Message::Ed2kRemoveServer(idx) => {
                if idx < self.config.ed2k.servers.len() {
                    self.config.ed2k.servers.remove(idx);
                    return self.save_config_async();
                }
            }
            Message::Ed2kResetDefaultServers => {
                self.config.ed2k.servers = default_ed2k_servers();
                self.ed2k_server_tests.clear();
                self.toast = Some("已恢复默认 eD2k 服务器列表".to_string());
                return self.save_config_async();
            }
            Message::Ed2kTestServer(idx) => {
                if let Some(srv) = self.config.ed2k.servers.get(idx).cloned() {
                    let key = format!("{}:{}", srv.host, srv.port);
                    self.ed2k_server_tests
                        .insert(key.clone(), ServerTestStatus::Testing);
                    let nick = self.config.ed2k.nickname.clone();
                    let tcp_port = self.config.ed2k.tcp_port;
                    return Task::perform(
                        async move {
                            let res = test_ed2k_server(&srv.host, srv.port, &nick, tcp_port).await;
                            (key, res)
                        },
                        |(k, r)| Message::Ed2kTestServerFinished(k, r),
                    );
                }
            }
            Message::Ed2kTestServerFinished(key, res) => {
                match res {
                    Ok(ms) => {
                        self.ed2k_server_tests
                            .insert(key, ServerTestStatus::Online(ms));
                    }
                    Err(e) => {
                        self.ed2k_server_tests
                            .insert(key, ServerTestStatus::Offline(e));
                    }
                }
            }
            Message::Ed2kSave => {
                if self.config.ed2k.nickname.trim().is_empty() {
                    self.config.ed2k.nickname = "Siphon".to_string();
                }
                self.config.ed2k.tcp_port = self
                    .ed2k_tcp_port_input
                    .trim()
                    .parse::<u16>()
                    .unwrap_or(4662)
                    .max(1);
                self.config.ed2k.udp_port = self
                    .ed2k_udp_port_input
                    .trim()
                    .parse::<u16>()
                    .unwrap_or(4672)
                    .max(1);
                self.config.ed2k.max_sources_per_file = self
                    .ed2k_max_sources_input
                    .trim()
                    .parse::<u32>()
                    .unwrap_or(400)
                    .clamp(20, 5000);
                self.config.ed2k.upload_limit_kbps = self
                    .ed2k_upload_limit_input
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(0);
                self.ed2k_tcp_port_input = self.config.ed2k.tcp_port.to_string();
                self.ed2k_udp_port_input = self.config.ed2k.udp_port.to_string();
                self.ed2k_max_sources_input = self.config.ed2k.max_sources_per_file.to_string();
                self.ed2k_upload_limit_input = self.config.ed2k.upload_limit_kbps.to_string();
                self.toast = Some("eD2k 电驴协议设置已保存".to_string());
                return self.save_config_async();
            }

            // Rules workspace
            Message::RuleDraftName(v) => self.rule_draft.name = v,
            Message::RuleDraftExtensions(v) => self.rule_draft.extensions = v,
            Message::RuleDraftDomain(v) => self.rule_draft.domain_glob = v,
            Message::RuleDraftPriority(v) => self.rule_draft.priority = v,
            Message::RuleDraftSaveDir(v) => self.rule_draft.save_dir_template = v,
            Message::RuleDraftBrowseDir => {
                let raw_tpl = self.rule_draft.save_dir_template.trim().to_string();
                let base_dir = self.config.default_save_dir.clone();
                let candidate = if raw_tpl.is_empty() {
                    base_dir.clone()
                } else if raw_tpl.contains("{base}") {
                    PathBuf::from(raw_tpl.replace("{base}", &base_dir.to_string_lossy()))
                } else {
                    PathBuf::from(&raw_tpl)
                };
                let start = if candidate.is_dir() {
                    candidate
                } else {
                    base_dir
                };
                return Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new();
                        if start.is_dir() {
                            dlg = dlg.set_directory(start);
                        }
                        dlg.pick_folder().await.map(|h| h.path().to_path_buf())
                    },
                    Message::RuleDraftDirPicked,
                );
            }
            Message::RuleDraftDirPicked(Some(dir)) => {
                self.rule_draft.save_dir_template = dir.to_string_lossy().to_string();
            }
            Message::RuleDraftDirPicked(None) => {}
            Message::RuleDraftConnections(v) => self.rule_draft.connections = v,
            Message::RuleEditStart(id) => {
                if let Some(rule) = self.rules.iter().find(|r| r.id == id) {
                    let mut exts = Vec::new();
                    let mut domain_glob = String::new();
                    for cond in &rule.conditions {
                        match cond {
                            RuleCondition::Extensions(list) => exts.extend(list.iter().cloned()),
                            RuleCondition::DomainGlob(g) if domain_glob.is_empty() => {
                                domain_glob = g.clone();
                            }
                            _ => {}
                        }
                    }
                    self.rule_draft = RuleDraft {
                        editing_id: Some(rule.id),
                        name: rule.name.clone(),
                        extensions: exts.join(", "),
                        domain_glob,
                        priority: rule.priority.to_string(),
                        save_dir_template: rule
                            .action
                            .save_dir_template
                            .clone()
                            .unwrap_or_default(),
                        connections: rule
                            .action
                            .connections_override
                            .map(|c| c.to_string())
                            .unwrap_or_default(),
                    };
                    self.active_input = InputTarget::RuleName;
                    return operation::focus(views::rules_view::rule_name_input_id());
                }
            }
            Message::RuleEditCancel => {
                self.rule_draft = RuleDraft {
                    save_dir_template: r"{base}\{domain}".to_string(),
                    ..Default::default()
                };
            }
            Message::RuleAddSubmit => {
                let mut conditions = Vec::new();
                let exts: Vec<String> = self
                    .rule_draft
                    .extensions
                    .split([',', ';', ' '])
                    .map(|s| s.trim().trim_start_matches('.').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !exts.is_empty() {
                    conditions.push(RuleCondition::Extensions(exts));
                }
                let dom = self.rule_draft.domain_glob.trim().to_string();
                if !dom.is_empty() {
                    conditions.push(RuleCondition::DomainGlob(dom));
                }

                let save_dir_template = {
                    let s = self.rule_draft.save_dir_template.trim();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.to_string())
                    }
                };
                let connections_override = self
                    .rule_draft
                    .connections
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .map(|c| c.clamp(1, 32));
                let parsed_priority = self.rule_draft.priority.trim().parse::<i32>().ok();

                if let Some(edit_id) = self.rule_draft.editing_id {
                    if let Some(existing) = self.rules.iter_mut().find(|r| r.id == edit_id) {
                        // Preserve any non-form conditions (e.g. MimePrefix, DomainRegex, Size, Source)
                        let preserved: Vec<RuleCondition> = existing
                            .conditions
                            .iter()
                            .filter(|c| {
                                !matches!(
                                    c,
                                    RuleCondition::Extensions(_) | RuleCondition::DomainGlob(_)
                                )
                            })
                            .cloned()
                            .collect();
                        conditions.extend(preserved);
                        if conditions.is_empty() {
                            return Task::none();
                        }
                        existing.name = self.rule_draft.name.trim().to_string();
                        if let Some(p) = parsed_priority {
                            existing.priority = p;
                        }
                        let has_ext = conditions
                            .iter()
                            .any(|c| matches!(c, RuleCondition::Extensions(_)));
                        let has_dom = conditions
                            .iter()
                            .any(|c| matches!(c, RuleCondition::DomainGlob(_)));
                        existing.match_all = has_ext && has_dom;
                        existing.conditions = conditions;
                        existing.action.save_dir_template = save_dir_template;
                        existing.action.connections_override = connections_override;
                    }
                    self.rules.sort_by_key(|r| r.priority);
                    self.toast = Some("规则已更新".to_string());
                } else {
                    if conditions.is_empty() {
                        return Task::none();
                    }
                    let next_priority = parsed_priority.unwrap_or_else(|| {
                        (self
                            .rules
                            .iter()
                            .map(|r| r.priority)
                            .min()
                            .unwrap_or(10)
                            - 1)
                        .max(1)
                    });

                    let new_rule = AutomationRule {
                        id: Uuid::new_v4(),
                        name: self.rule_draft.name.trim().to_string(),
                        enabled: true,
                        priority: next_priority,
                        stop_on_match: true,
                        match_all: conditions.len() > 1,
                        conditions,
                        action: RuleAction {
                            save_dir_template,
                            connections_override,
                            ..Default::default()
                        },
                    };
                    self.rules.insert(0, new_rule);
                    self.rules.sort_by_key(|r| r.priority);
                    self.toast = Some("已添加新规则".to_string());
                }

                self.rule_draft = RuleDraft {
                    save_dir_template: r"{base}\{domain}".to_string(),
                    ..Default::default()
                };
                let rules_clone = self.rules.clone();
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        let _ = mgr.save_rules(rules_clone).await;
                        mgr.list_rules().await
                    },
                    Message::RulesLoaded,
                );
            }
            Message::RuleToggleEnabled(id) => {
                if let Some(r) = self.rules.iter_mut().find(|r| r.id == id) {
                    r.enabled = !r.enabled;
                }
                let rules_clone = self.rules.clone();
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        let _ = mgr.save_rules(rules_clone).await;
                        mgr.list_rules().await
                    },
                    Message::RulesLoaded,
                );
            }
            Message::RuleDelete(id) => {
                self.rules.retain(|r| r.id != id);
                if self.rule_draft.editing_id == Some(id) {
                    self.rule_draft = RuleDraft {
                        save_dir_template: r"{base}\{domain}".to_string(),
                        ..Default::default()
                    };
                }
                let rules_clone = self.rules.clone();
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        let _ = mgr.save_rules(rules_clone).await;
                        mgr.list_rules().await
                    },
                    Message::RulesLoaded,
                );
            }
            Message::RulesLoaded(rules) => {
                self.rules = rules;
            }

            // Plugins workspace
            Message::ReloadPlugins => {
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move { mgr.reload_plugins().await },
                    Message::PluginsLoaded,
                );
            }
            Message::PluginsLoaded(list) => {
                self.plugins = list;
                self.toast = Some(format!("已加载 {} 个外部插件", self.plugins.len()));
            }
            Message::TogglePlugin(name, enabled) => {
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        let _ = mgr.set_plugin_enabled(&name, enabled).await;
                        mgr.list_plugins().await
                    },
                    Message::PluginsLoaded,
                );
            }
            Message::OpenPluginsFolder => {
                let dir = AppConfig::plugins_dir(self.manager.data_dir());
                let _ = std::fs::create_dir_all(&dir);
                open_folder(&dir);
            }
            Message::InstallExternalPlugin => {
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("选择包含 plugin.toml 的插件目录")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::ExternalPluginDirPicked,
                );
            }
            Message::ExternalPluginDirPicked(Some(src_dir)) => {
                let manifest_path = src_dir.join("plugin.toml");
                if !manifest_path.is_file() {
                    self.toast = Some("所选目录未找到 plugin.toml，请选择有效的插件目录".to_string());
                    return Task::none();
                }
                let folder_name = src_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("plugin-{}", Uuid::new_v4()));
                let plugins_root = AppConfig::plugins_dir(self.manager.data_dir());
                let dst_dir = plugins_root.join(&folder_name);
                if src_dir != dst_dir
                    && let Err(e) = copy_dir_recursive(&src_dir, &dst_dir)
                {
                    self.toast = Some(format!("导入插件失败: {e}"));
                    return Task::none();
                }
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move { mgr.reload_plugins().await },
                    Message::PluginsLoaded,
                );
            }
            Message::ExternalPluginDirPicked(None) => {}

            // Settings workspace
            Message::SetThemeMode(mode) => {
                self.config.theme_mode = mode;
                self.apply_backdrop_now();
                return self.save_config_async();
            }
            Message::SetAccentPreset(hex) => {
                if let Some(norm) = normalize_hex_color(&hex) {
                    self.config.accent_color = norm.clone();
                    self.accent_hex_input = norm;
                    return self.save_config_async();
                }
            }
            Message::SetAccentHexInput(v) => {
                self.accent_hex_input = v;
            }
            Message::ApplyAccentHex => {
                if let Some(norm) = normalize_hex_color(&self.accent_hex_input) {
                    self.config.accent_color = norm.clone();
                    self.accent_hex_input = norm;
                    self.toast = Some("主界面主题色已更新".to_string());
                    return self.save_config_async();
                } else {
                    self.toast = Some("色值格式无效，请输入 6 位 HEX 色值（如 #38BDF8）".to_string());
                }
            }
            Message::SetDownloadPopupAccentPreset(hex) => {
                if let Some(norm) = normalize_hex_color(&hex) {
                    self.config.download_popup_accent = norm.clone();
                    self.popup_accent_hex_input = norm;
                    return self.save_config_async();
                }
            }
            Message::SetDownloadPopupAccentHexInput(v) => {
                self.popup_accent_hex_input = v;
            }
            Message::ApplyDownloadPopupAccentHex => {
                if let Some(norm) = normalize_hex_color(&self.popup_accent_hex_input) {
                    self.config.download_popup_accent = norm.clone();
                    self.popup_accent_hex_input = norm;
                    self.toast = Some("下载弹窗数据色已更新".to_string());
                    return self.save_config_async();
                } else {
                    self.toast = Some("色值格式无效，请输入 6 位 HEX 色值（如 #22C55E）".to_string());
                }
            }
            Message::ToggleMica(v) => {
                self.config.use_mica = v;
                self.apply_backdrop_now();
                return self.save_config_async();
            }
            Message::SetDensity(d) => {
                self.config.list_density = d;
                return self.save_config_async();
            }
            Message::SetLanguage(lang) => {
                self.config.language = lang;
                return self.save_config_async();
            }
            Message::SetDefaultSaveDirInput(v) => {
                self.save_dir_input = v;
            }
            Message::BrowseDefaultSaveDir => {
                let start = PathBuf::from(&self.save_dir_input);
                return Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new();
                        if start.is_dir() {
                            dlg = dlg.set_directory(start);
                        }
                        dlg.pick_folder().await.map(|h| h.path().to_path_buf())
                    },
                    Message::DefaultSaveDirPicked,
                );
            }
            Message::DefaultSaveDirPicked(Some(dir)) => {
                self.save_dir_input = dir.to_string_lossy().to_string();
                self.config.default_save_dir = dir;
                return self.save_config_async();
            }
            Message::DefaultSaveDirPicked(None) => {}
            Message::SetDefaultConnections(v) => {
                self.config.default_connections = (v.round() as u32).clamp(1, 32);
            }
            Message::SetMaxConcurrentTasks(v) => {
                self.config.max_concurrent_tasks = (v.round() as usize).clamp(1, 16);
            }
            Message::SetMinSplitMiB(mib) => {
                self.config.min_split_size_bytes = mib.max(1) * 1024 * 1024;
                return self.save_config_async();
            }
            Message::SetGlobalSpeedLimitInput(v) => {
                self.global_speed_input = v;
            }
            Message::ApplyGlobalSpeedLimit => {
                let kbps = self.global_speed_input.trim().parse::<u64>().unwrap_or(0);
                let bps = kbps.saturating_mul(1024);
                self.config.global_speed_limit_bps = bps;
                let mgr = Arc::clone(&self.manager);
                return Task::perform(
                    async move {
                        mgr.update_global_speed_limit(bps)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::TaskOpFinished,
                );
            }
            Message::SetProxyUrlInput(v) => self.proxy_input = v,
            Message::SetUserAgentInput(v) => self.ua_input = v,
            Message::ToggleClipboardMonitor(v) => {
                self.config.clipboard_monitor = v;
                self.clipboard_flag.store(v, Ordering::Relaxed);
                return self.save_config_async();
            }
            Message::ToggleMinimizeToTray(v) => {
                self.config.minimize_to_tray = v;
                return self.save_config_async();
            }
            Message::ToggleAutostart(v) => {
                self.config.autostart = v;
                if let Err(e) = set_autostart(v) {
                    self.toast = Some(format!("设置开机自启失败: {e}"));
                }
                return self.save_config_async();
            }
            Message::ToggleConfirmOnDelete(v) => {
                self.config.confirm_on_delete = v;
                return self.save_config_async();
            }
            Message::SetDeleteFilesByDefault(v) => {
                self.config.delete_files_by_default = v;
                return self.save_config_async();
            }
            Message::ToggleBrowserIntegration(v) => {
                self.config.browser_integration = v;
                self.browser_flag.store(v, Ordering::Relaxed);
                return self.save_config_async();
            }
            Message::ToggleBrowserCapturePopup(v) => {
                self.config.browser_capture_popup = v;
                return self.save_config_async();
            }
            Message::ToggleClipboardCapturePopup(v) => {
                self.config.clipboard_capture_popup = v;
                return self.save_config_async();
            }
            Message::OpenBrowserExtensionFolder => {
                let deployed = self.manager.data_dir().join("browser-extension");
                if deployed.is_dir() {
                    open_folder(&deployed);
                } else {
                    let mut ext_dir = std::env::current_dir()
                        .unwrap_or_else(|_| PathBuf::from("."))
                        .join("browser-extension");
                    if !ext_dir.is_dir()
                        && let Ok(exe) = std::env::current_exe()
                        && let Some(parent) = exe.parent()
                    {
                        let beside = parent.join("browser-extension");
                        if beside.is_dir() {
                            ext_dir = beside;
                        }
                    }
                    open_folder(&ext_dir);
                }
            }
            Message::InstallBrowserExtension => {
                let browsers = detect_installed_browsers();
                let selected_browser_id = browsers
                    .first()
                    .map(|b| b.id.clone())
                    .unwrap_or_default();
                let (ext_dir, deployed, error_msg) =
                    match deploy_browser_extension(self.manager.data_dir()) {
                        Ok(dir) => (dir, true, None),
                        Err(e) => (
                            self.manager.data_dir().join("browser-extension"),
                            false,
                            Some(format!("部署扩展文件失败: {e}")),
                        ),
                    };
                self.ext_install_modal = Some(ExtInstallModalState {
                    browsers,
                    selected_browser_id,
                    ext_dir,
                    deployed,
                    path_copied: false,
                    browser_launched: false,
                    error_msg,
                });
            }
            Message::CloseExtInstallModal => {
                self.ext_install_modal = None;
            }
            Message::ExtInstallSelectBrowser(id) => {
                if let Some(m) = &mut self.ext_install_modal {
                    m.selected_browser_id = id;
                }
            }
            Message::ExtInstallCopyPath => {
                if let Some(m) = &mut self.ext_install_modal {
                    let path_str = m.ext_dir.to_string_lossy().to_string();
                    if write_clipboard_text(&path_str) {
                        m.path_copied = true;
                        self.toast = Some("已复制浏览器扩展目录路径".to_string());
                    }
                }
            }
            Message::ExtInstallRevealFolder => {
                if let Some(m) = &self.ext_install_modal {
                    reveal_in_explorer(&m.ext_dir);
                }
            }
            Message::ExtInstallLaunchSelectedBrowser => {
                if let Some(m) = &mut self.ext_install_modal {
                    let selected = m
                        .browsers
                        .iter()
                        .find(|b| b.id == m.selected_browser_id)
                        .cloned();
                    launch_browser_extensions_page(selected.as_ref(), &m.ext_dir);
                    m.path_copied = true;
                    m.browser_launched = true;
                }
            }
            Message::ExportBrowserExtensionZip => {
                let default_dir = self.config.default_save_dir.clone();
                return Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .set_title("导出 Siphon 浏览器扩展压缩包 (.zip)")
                            .set_file_name(format!(
                                "siphon-browser-extension-v{}.zip",
                                views::about_view::APP_VERSION
                            ))
                            .add_filter("ZIP 压缩包", &["zip"]);
                        if default_dir.is_dir() {
                            dlg = dlg.set_directory(default_dir);
                        }
                        dlg.save_file().await.map(|h| h.path().to_path_buf())
                    },
                    Message::BrowserExtensionZipSaved,
                );
            }
            Message::BrowserExtensionZipSaved(Some(dest)) => {
                match export_browser_extension_zip(&dest) {
                    Ok(saved) => {
                        reveal_in_explorer(&saved);
                        self.toast = Some(format!("已导出浏览器扩展压缩包: {}", saved.display()));
                    }
                    Err(e) => {
                        self.toast = Some(format!("导出扩展压缩包失败: {e}"));
                    }
                }
            }
            Message::BrowserExtensionZipSaved(None) => {}
            Message::OpenDataDir => {
                open_folder(self.manager.data_dir());
            }
            Message::ChangeDataDir => {
                let current = self.manager.data_dir().to_path_buf();
                return Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .set_title("选择新的 Siphon 数据存储目录");
                        if current.is_dir() {
                            dlg = dlg.set_directory(current);
                        }
                        dlg.pick_folder().await.map(|h| h.path().to_path_buf())
                    },
                    Message::DataDirPicked,
                );
            }
            Message::DataDirPicked(Some(new_dir)) => {
                let old_dir = self.manager.data_dir().to_path_buf();
                match AppConfig::migrate_data_dir(&old_dir, &new_dir) {
                    Ok(()) => {
                        self.toast = Some(format!(
                            "数据目录已迁移至 {}，重启 Siphon 后生效",
                            new_dir.display()
                        ));
                    }
                    Err(e) => {
                        self.toast = Some(format!("更改数据目录失败: {e}"));
                    }
                }
            }
            Message::DataDirPicked(None) => {}
            Message::CheckForUpdates => {
                self.update_status = UpdateCheckStatus::Checking;
                let proxy = self.config.proxy_url.clone();
                return Task::perform(
                    async move {
                        check_for_updates(views::about_view::APP_VERSION, proxy.as_deref()).await
                    },
                    Message::UpdateCheckFinished,
                );
            }
            Message::UpdateCheckFinished(res) => match res {
                Ok(info) => {
                    if info.has_update {
                        self.toast = Some(format!("发现 Siphon 新版本 v{}", info.latest_version));
                        self.update_status = UpdateCheckStatus::UpdateAvailable(info);
                    } else {
                        self.toast = Some(format!("当前已是最新版本 v{}", info.current_version));
                        self.update_status = UpdateCheckStatus::UpToDate(info.current_version);
                    }
                }
                Err(e) => {
                    self.update_status = UpdateCheckStatus::Failed(e);
                }
            },
            Message::OpenExternalUrl(url) => {
                open_url(&url);
            }
            Message::DownloadUpdateInstaller(url) => {
                self.active_nav = NavSection::All;
                let mgr = Arc::clone(&self.manager);
                let req = NewTaskRequest::quick(url, TaskSource::Manual);
                return Task::perform(
                    async move { mgr.add_task(req).await.map_err(|e| e.to_string()) },
                    Message::CaptureStartedWithDialog,
                );
            }
            Message::StartAutoUpdate(url) => {
                let update_dir = std::env::temp_dir().join("SiphonUpdate");
                let _ = std::fs::create_dir_all(&update_dir);
                let mgr = Arc::clone(&self.manager);
                let req = NewTaskRequest {
                    url,
                    filename: None,
                    save_dir: Some(update_dir),
                    max_connections: Some(8),
                    speed_limit_bps: Some(0),
                    headers: BTreeMap::new(),
                    checksum: None,
                    source: TaskSource::Manual,
                    auto_start: true,
                };
                self.toast = Some("开始下载更新安装包，完成后将自动启动静默升级...".to_string());
                return Task::perform(
                    async move { mgr.add_task(req).await.map_err(|e| e.to_string()) },
                    Message::AutoUpdateTaskStarted,
                );
            }
            Message::AutoUpdateTaskStarted(res) => match res {
                Ok(task) => {
                    let task_id = task.id;
                    self.auto_update_task_id = Some(task_id);
                    self.tasks.retain(|t| t.id != task_id);
                    self.tasks.insert(0, task);
                }
                Err(e) => {
                    self.auto_update_task_id = None;
                    self.toast = Some(format!("下载更新包失败: {e}"));
                }
            },
            Message::SaveSettings => {
                if !self.save_dir_input.trim().is_empty() {
                    self.config.default_save_dir = PathBuf::from(self.save_dir_input.trim());
                }
                let kbps = self.global_speed_input.trim().parse::<u64>().unwrap_or(0);
                self.config.global_speed_limit_bps = kbps.saturating_mul(1024);
                self.config.proxy_url = {
                    let p = self.proxy_input.trim();
                    if p.is_empty() {
                        None
                    } else {
                        Some(p.to_string())
                    }
                };
                if !self.ua_input.trim().is_empty() {
                    self.config.user_agent = self.ua_input.trim().to_string();
                }
                if let Some(norm) = normalize_hex_color(&self.accent_hex_input) {
                    self.config.accent_color = norm.clone();
                    self.accent_hex_input = norm;
                }
                if let Some(norm) = normalize_hex_color(&self.popup_accent_hex_input) {
                    self.config.download_popup_accent = norm.clone();
                    self.popup_accent_hex_input = norm;
                }
                self.toast = Some("系统设置已保存".to_string());
                return self.save_config_async();
            }
            Message::SettingsSaved(Ok(cfg)) => {
                self.config = cfg;
            }
            Message::SettingsSaved(Err(e)) => {
                self.toast = Some(format!("保存配置失败: {e}"));
            }
            Message::ClearToast => {
                self.toast = None;
            }
        }

        Task::none()
    }

    fn get_input_value(&self, target: InputTarget) -> &str {
        match target {
            InputTarget::QuickBar => &self.quick_input,
            InputTarget::ModalUrl => &self.modal.url,
            InputTarget::ModalFilename => &self.modal.filename,
            InputTarget::ModalSaveDir => &self.modal.save_dir,
            InputTarget::ModalSpeedLimit => &self.modal.speed_limit_kbps,
            InputTarget::ModalSha256 => &self.modal.sha256,
            InputTarget::ModalBatchUrls => &self.modal.batch_text,
            InputTarget::ModalBatchFilter => &self.modal.batch_filter,
            InputTarget::ReplaceUrl => &self.replace_url_input,
            InputTarget::RuleName => &self.rule_draft.name,
            InputTarget::RuleExtensions => &self.rule_draft.extensions,
            InputTarget::RuleDomain => &self.rule_draft.domain_glob,
            InputTarget::RulePriority => &self.rule_draft.priority,
            InputTarget::RuleSaveDir => &self.rule_draft.save_dir_template,
            InputTarget::RuleConnections => &self.rule_draft.connections,
            InputTarget::SettingsSaveDir => &self.save_dir_input,
            InputTarget::SettingsGlobalSpeed => &self.global_speed_input,
            InputTarget::SettingsProxy => &self.proxy_input,
            InputTarget::SettingsUserAgent => &self.ua_input,
            InputTarget::SettingsAccentHex => &self.accent_hex_input,
            InputTarget::SettingsPopupAccentHex => &self.popup_accent_hex_input,
            InputTarget::SchedStartTime => &self.config.schedule.start_time,
            InputTarget::SchedStopTime => &self.config.schedule.stop_time,
            InputTarget::SchedSpeedStart => &self.config.schedule.speed_start_time,
            InputTarget::SchedSpeedEnd => &self.config.schedule.speed_end_time,
            InputTarget::SchedSpeedKbps => &self.sched_speed_kbps_input,
            InputTarget::SchedScript => &self.config.schedule.post_queue_script,
            InputTarget::BtListenPort => &self.bt_listen_port_input,
            InputTarget::BtMaxPeers => &self.bt_max_peers_input,
            InputTarget::BtConnectTimeout => &self.bt_timeout_input,
            InputTarget::BtNewTracker => &self.bt_new_tracker_input,
            InputTarget::BtNewSubscribeUrl => &self.bt_new_sub_url_input,
            InputTarget::Ed2kNickname => &self.config.ed2k.nickname,
            InputTarget::Ed2kTcpPort => &self.ed2k_tcp_port_input,
            InputTarget::Ed2kUdpPort => &self.ed2k_udp_port_input,
            InputTarget::Ed2kMaxSources => &self.ed2k_max_sources_input,
            InputTarget::Ed2kUploadLimit => &self.ed2k_upload_limit_input,
            InputTarget::Ed2kServerMetUrl => &self.config.ed2k.server_met_url,
            InputTarget::Ed2kQuickServerLink => &self.ed2k_quick_server_link,
            InputTarget::Ed2kNewServerName => &self.ed2k_new_srv_name,
            InputTarget::Ed2kNewServerHost => &self.ed2k_new_srv_host,
            InputTarget::Ed2kNewServerPort => &self.ed2k_new_srv_port,
            InputTarget::CaptureFilename => self
                .capture_popup
                .as_ref()
                .map(|p| p.filename.as_str())
                .unwrap_or(""),
            InputTarget::CaptureSaveDir => self
                .capture_popup
                .as_ref()
                .map(|p| p.save_dir.as_str())
                .unwrap_or(""),
            InputTarget::SniffPageUrl => self
                .sniff_popup
                .as_ref()
                .map(|s| s.page_url_input.as_str())
                .unwrap_or(""),
            InputTarget::SniffSearch => self
                .sniff_popup
                .as_ref()
                .map(|s| s.search.as_str())
                .unwrap_or(""),
            InputTarget::SniffSaveDir => self
                .sniff_popup
                .as_ref()
                .map(|s| s.save_dir.as_str())
                .unwrap_or(""),
        }
    }

    fn set_input_value(&mut self, target: InputTarget, val: String) {
        match target {
            InputTarget::QuickBar => self.quick_input = val,
            InputTarget::ModalUrl => {
                self.modal.url = val;
                self.modal.probe_error = None;
            }
            InputTarget::ModalFilename => self.modal.filename = val,
            InputTarget::ModalSaveDir => self.modal.save_dir = val,
            InputTarget::ModalSpeedLimit => self.modal.speed_limit_kbps = val,
            InputTarget::ModalSha256 => self.modal.sha256 = val,
            InputTarget::ModalBatchUrls => self.modal.batch_text = val,
            InputTarget::ModalBatchFilter => self.modal.batch_filter = val,
            InputTarget::ReplaceUrl => self.replace_url_input = val,
            InputTarget::RuleName => self.rule_draft.name = val,
            InputTarget::RuleExtensions => self.rule_draft.extensions = val,
            InputTarget::RuleDomain => self.rule_draft.domain_glob = val,
            InputTarget::RulePriority => self.rule_draft.priority = val,
            InputTarget::RuleSaveDir => self.rule_draft.save_dir_template = val,
            InputTarget::RuleConnections => self.rule_draft.connections = val,
            InputTarget::SettingsSaveDir => self.save_dir_input = val,
            InputTarget::SettingsGlobalSpeed => self.global_speed_input = val,
            InputTarget::SettingsProxy => self.proxy_input = val,
            InputTarget::SettingsUserAgent => self.ua_input = val,
            InputTarget::SettingsAccentHex => self.accent_hex_input = val,
            InputTarget::SettingsPopupAccentHex => self.popup_accent_hex_input = val,
            InputTarget::SchedStartTime => self.config.schedule.start_time = val,
            InputTarget::SchedStopTime => self.config.schedule.stop_time = val,
            InputTarget::SchedSpeedStart => self.config.schedule.speed_start_time = val,
            InputTarget::SchedSpeedEnd => self.config.schedule.speed_end_time = val,
            InputTarget::SchedSpeedKbps => {
                self.config.schedule.scheduled_speed_limit_kbps =
                    val.trim().parse::<u64>().unwrap_or(0);
                self.sched_speed_kbps_input = val;
            }
            InputTarget::SchedScript => self.config.schedule.post_queue_script = val,
            InputTarget::BtListenPort => self.bt_listen_port_input = val,
            InputTarget::BtMaxPeers => self.bt_max_peers_input = val,
            InputTarget::BtConnectTimeout => self.bt_timeout_input = val,
            InputTarget::BtNewTracker => self.bt_new_tracker_input = val,
            InputTarget::BtNewSubscribeUrl => self.bt_new_sub_url_input = val,
            InputTarget::Ed2kNickname => self.config.ed2k.nickname = val,
            InputTarget::Ed2kTcpPort => self.ed2k_tcp_port_input = val,
            InputTarget::Ed2kUdpPort => self.ed2k_udp_port_input = val,
            InputTarget::Ed2kMaxSources => self.ed2k_max_sources_input = val,
            InputTarget::Ed2kUploadLimit => self.ed2k_upload_limit_input = val,
            InputTarget::Ed2kServerMetUrl => self.config.ed2k.server_met_url = val,
            InputTarget::Ed2kQuickServerLink => self.ed2k_quick_server_link = val,
            InputTarget::Ed2kNewServerName => self.ed2k_new_srv_name = val,
            InputTarget::Ed2kNewServerHost => self.ed2k_new_srv_host = val,
            InputTarget::Ed2kNewServerPort => self.ed2k_new_srv_port = val,
            InputTarget::CaptureFilename => {
                if let Some(popup) = &mut self.capture_popup {
                    popup.filename = val;
                    popup.filename_edited_by_user = true;
                }
            }
            InputTarget::CaptureSaveDir => {
                if let Some(popup) = &mut self.capture_popup {
                    popup.save_dir = val;
                    popup.save_dir_edited_by_user = true;
                }
            }
            InputTarget::SniffPageUrl => {
                if let Some(s) = &mut self.sniff_popup {
                    s.page_url_input = val;
                    s.sniff_error = None;
                }
            }
            InputTarget::SniffSearch => {
                if let Some(s) = &mut self.sniff_popup {
                    s.search = val;
                }
            }
            InputTarget::SniffSaveDir => {
                if let Some(s) = &mut self.sniff_popup {
                    s.save_dir = val;
                }
            }
        }
    }

    fn paste_clipboard_into(&mut self, target: InputTarget, trigger_action: bool) -> Task<Message> {
        let Some(raw) = read_clipboard_text() else {
            return Task::none();
        };
        let cleaned: String = if target == InputTarget::ModalBatchUrls {
            raw.replace(['\r', '\n'], " ").trim().to_string()
        } else {
            raw.chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .to_string()
        };
        if cleaned.is_empty() {
            return Task::none();
        }

        let is_url_box = matches!(
            target,
            InputTarget::QuickBar
                | InputTarget::ModalUrl
                | InputTarget::ReplaceUrl
                | InputTarget::SniffPageUrl
        );
        let low = cleaned.to_ascii_lowercase();
        let pasted_is_url = low.starts_with("http://")
            || low.starts_with("https://")
            || low.starts_with("magnet:?")
            || low.starts_with("ed2k://|file|");

        let current = self.get_input_value(target).to_string();
        let next_val = if current.is_empty() || (is_url_box && pasted_is_url) {
            cleaned
        } else {
            format!("{current}{cleaned}")
        };
        self.set_input_value(target, next_val);
        self.active_input = target;

        if trigger_action {
            match target {
                InputTarget::QuickBar => return self.update(Message::QuickBarSubmit),
                InputTarget::ModalUrl => return self.update(Message::ModalProbeUrl),
                InputTarget::SniffPageUrl => return self.update(Message::SniffFetchPageUrl),
                _ => {}
            }
        }

        match target {
            InputTarget::QuickBar => operation::focus(views::titlebar::quick_bar_id()),
            InputTarget::ModalUrl => operation::focus(views::new_task::modal_url_id()),
            InputTarget::ReplaceUrl => operation::focus(views::inspector::replace_url_id()),
            InputTarget::SniffPageUrl => operation::focus(views::sniff_popup::sniff_page_url_id()),
            _ => Task::none(),
        }
    }

    fn view_context_menu(&self, menu: ContextMenuState, palette: SiphonPalette) -> Element<'_, Message> {
        let target = menu.target;
        let has_value = !self.get_input_value(target).is_empty();

        let item = |label: &'static str, shortcut: &'static str, msg: Option<Message>| {
            let content = row![
                text(label)
                    .size(13)
                    .font(FONT_REGULAR)
                    .color(if msg.is_some() {
                        palette.text_primary
                    } else {
                        palette.text_muted
                    }),
                space::horizontal(),
                text(shortcut)
                    .size(11)
                    .font(FONT_MONO)
                    .color(palette.text_muted),
            ]
            .align_y(Alignment::Center);

            button(content)
                .width(Length::Fill)
                .padding([7, 10])
                .style(subtle_btn_style(palette, false))
                .on_press_maybe(msg)
        };

        let mut items = column![item("粘贴", "Ctrl V", Some(Message::ContextPaste(target)))].spacing(2);

        if target == InputTarget::QuickBar {
            items = items.push(item(
                "粘贴并极速下载",
                "",
                Some(Message::ContextPasteAndGo(target)),
            ));
        } else if target == InputTarget::ModalUrl {
            items = items.push(item(
                "粘贴并探测信息",
                "",
                Some(Message::ContextPasteAndGo(target)),
            ));
        } else if target == InputTarget::SniffPageUrl {
            items = items.push(item(
                "粘贴并嗅探网页",
                "",
                Some(Message::ContextPasteAndGo(target)),
            ));
        }

        items = items
            .push(rule::horizontal(1).style(hairline_rule_style(palette)))
            .push(item(
                "复制内容",
                "Ctrl C",
                has_value.then_some(Message::ContextCopy(target)),
            ))
            .push(item(
                "清空输入",
                "",
                has_value.then_some(Message::ContextClear(target)),
            ));

        let menu_w = 184.0f32;
        let menu_h = 156.0f32;
        let (win_w, win_h) = if matches!(
            target,
            InputTarget::CaptureFilename | InputTarget::CaptureSaveDir
        ) {
            (620.0f32, 404.0f32)
        } else if matches!(
            target,
            InputTarget::SniffPageUrl | InputTarget::SniffSearch | InputTarget::SniffSaveDir
        ) {
            (780.0f32, 580.0f32)
        } else {
            (self.window_size.width, self.window_size.height)
        };
        let x = menu
            .position
            .x
            .clamp(8.0, (win_w - menu_w - 8.0).max(8.0));
        let y = menu
            .position
            .y
            .clamp(8.0, (win_h - menu_h - 8.0).max(8.0));

        let card = container(items)
            .width(Length::Fixed(menu_w))
            .padding(6)
            .style(modal_card_style(palette));

        mouse_area(
            container(mouse_area(card).on_press(Message::ClearToast))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(iced::Padding {
                    top: y,
                    right: 0.0,
                    bottom: 0.0,
                    left: x,
                }),
        )
        .on_press(Message::CloseContextMenu)
        .on_right_press(Message::CloseContextMenu)
        .into()
    }

    fn handle_dropped_urls(&mut self, urls: Vec<String>) -> Task<Message> {
        let mut unique = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for u in urls {
            if seen.insert(u.clone()) {
                unique.push(u);
            }
        }
        if unique.is_empty() {
            return Task::none();
        }
        if unique.len() == 1 {
            let url = unique.remove(0);
            if self.modal.open {
                self.modal.url = url;
                self.modal.probe_error = None;
                return self.update(Message::ModalProbeUrl);
            }
            return self.update(Message::OpenNewTaskModal(Some(url)));
        }

        let count = unique.len();
        self.toast = Some(format!("正在批量导入 {count} 个拖入链接..."));
        self.active_nav = NavSection::All;
        let mgr = Arc::clone(&self.manager);
        Task::perform(
            async move {
                let mut err_msg = None;
                for url in unique {
                    let req = NewTaskRequest::quick(url, TaskSource::DragDrop);
                    if let Err(e) = mgr.add_task(req).await {
                        err_msg = Some(e.to_string());
                    }
                }
                match err_msg {
                    Some(e) => Err(e),
                    None => Ok(()),
                }
            },
            Message::TaskOpFinished,
        )
    }

    fn save_config_async(&self) -> Task<Message> {
        let cfg = self.config.clone();
        let mgr = Arc::clone(&self.manager);
        Task::perform(
            async move {
                mgr.update_config(cfg.clone())
                    .await
                    .map(|_| cfg)
                    .map_err(|e| e.to_string())
            },
            Message::SettingsSaved,
        )
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let window_open = window::open_events().map(Message::WindowOpened);
        let window_close = window::close_requests().map(Message::WindowCloseRequested);
        let window_closed = window::close_events().map(Message::WindowClosed);

        let global_events = iced::event::listen_with(|event, status, id| match event {
            Event::Window(window::Event::FileDropped(path)) => Some(Message::FileDropped(path)),
            Event::Window(window::Event::Resized(size)) => Some(Message::WindowResized(id, size)),
            Event::Window(window::Event::Focused) => Some(Message::WindowFocused(id)),
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorMoved(id, position))
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) => {
                if modifiers == Modifiers::CTRL {
                    match key.as_ref() {
                        Key::Character("n") | Key::Character("N") => {
                            return Some(Message::ShortcutNewTask);
                        }
                        Key::Character("k") | Key::Character("K") => {
                            return Some(Message::ShortcutFocusQuickBar);
                        }
                        _ => {}
                    }
                }
                if matches!(key, Key::Named(keyboard::key::Named::Escape)) {
                    return Some(Message::ShortcutEscape);
                }

                // Fallback paste catcher when Iced's TextInput didn't capture Ctrl+V / Win+V / Shift+Insert
                if status == iced::event::Status::Ignored {
                    let is_ctrl_v = modifiers.control()
                        && !modifiers.alt()
                        && (matches!(
                            key.as_ref(),
                            Key::Character("v") | Key::Character("V") | Key::Character("\u{16}")
                        ) || matches!(
                            physical_key,
                            keyboard::key::Physical::Code(keyboard::key::Code::KeyV)
                        ));
                    let is_shift_ins = modifiers.shift()
                        && matches!(key, Key::Named(keyboard::key::Named::Insert));
                    if is_ctrl_v || is_shift_ins {
                        return Some(Message::PasteIntoActiveInput);
                    }
                }
                None
            }
            _ => None,
        });

        let runtime_hub = Subscription::run(runtime_event_stream);

        Subscription::batch([
            window_open,
            window_close,
            window_closed,
            global_events,
            runtime_hub,
        ])
    }

    pub fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        let palette = self.palette();

        // Render standalone capture floating window when `window_id` belongs to `capture_popup`
        if let Some(popup) = &self.capture_popup
            && popup.window_id == Some(window_id)
        {
            let icon_key = FileIconKey::for_task(&popup.filename, Path::new(""), false);
            let sys_icon = self.file_icons.get(&icon_key).map(|(_, h)| h);
            let popup_base = views::capture_popup::view(popup, sys_icon, palette);
            if let Some(menu) = self.context_menu
                && matches!(
                    menu.target,
                    InputTarget::CaptureFilename | InputTarget::CaptureSaveDir
                )
            {
                let menu_layer = self.view_context_menu(menu, palette);
                return stack![popup_base, menu_layer].into();
            }
            return popup_base;
        }

        // Render standalone Browser Resource Sniff & Selective Download window when `window_id` belongs to `sniff_popup`
        if let Some(sniff) = &self.sniff_popup
            && sniff.window_id == Some(window_id)
        {
            let sniff_base = views::sniff_popup::view(sniff, palette);
            if let Some(menu) = self.context_menu
                && matches!(
                    menu.target,
                    InputTarget::SniffPageUrl
                        | InputTarget::SniffSearch
                        | InputTarget::SniffSaveDir
                )
            {
                let menu_layer = self.view_context_menu(menu, palette);
                return stack![sniff_base, menu_layer].into();
            }
            return sniff_base;
        }

        // Render standalone download progress detail window when `window_id` belongs to `download_popups`
        if let Some(dp) = self.download_popups.get(&window_id) {
            let task = self.tasks.iter().find(|t| t.id == dp.task_id);
            let sys_icon = task.and_then(|t| {
                let icon_key = Self::task_icon_key(t);
                self.file_icons
                    .get(&icon_key)
                    .or_else(|| self.file_icons.get(&icon_key.fallback_extension()))
                    .map(|(_, h)| h)
            });
            return views::download_popup::view(
                dp,
                task,
                sys_icon,
                &self.config.download_popup_accent,
                palette,
            );
        }

        let strings = Strings::new(self.config.language);

        let titlebar = views::titlebar::view(&self.quick_input, palette, strings);
        let sidebar = views::sidebar::view(
            self.active_nav,
            &self.tasks,
            self.global_speed_bps,
            self.active_count,
            palette,
            strings,
        );

        let center_and_inspector: Element<'_, Message> = match self.active_nav {
            NavSection::BitTorrent => views::bt_view::view(
                &self.config.bt,
                &self.bt_listen_port_input,
                &self.bt_max_peers_input,
                &self.bt_timeout_input,
                &self.bt_new_tracker_input,
                &self.bt_new_sub_url_input,
                self.bt_syncing_trackers,
                self.bt_sync_status.as_deref(),
                palette,
            ),
            NavSection::Ed2k => views::ed2k_view::view(
                &self.config.ed2k,
                &self.ed2k_tcp_port_input,
                &self.ed2k_udp_port_input,
                &self.ed2k_max_sources_input,
                &self.ed2k_upload_limit_input,
                &self.ed2k_quick_server_link,
                &self.ed2k_new_srv_name,
                &self.ed2k_new_srv_host,
                &self.ed2k_new_srv_port,
                self.ed2k_syncing_met,
                self.ed2k_met_sync_status.as_deref(),
                &self.ed2k_server_tests,
                palette,
            ),
            NavSection::Scheduler => views::scheduler_view::view(
                &self.config.schedule,
                &self.sched_speed_kbps_input,
                self.sched_speed_window_active,
                palette,
            ),
            NavSection::Rules => views::rules_view::view(&self.rules, &self.rule_draft, palette),
            NavSection::Plugins => views::plugins_view::view(
                &self.plugins,
                &AppConfig::plugins_dir(self.manager.data_dir()),
                palette,
            ),
            NavSection::Settings => views::settings::view(
                &self.config,
                &self.save_dir_input,
                &self.global_speed_input,
                &self.proxy_input,
                &self.ua_input,
                &self.accent_hex_input,
                &self.popup_accent_hex_input,
                self.browser_ext_connected,
                palette,
            ),
            NavSection::About => {
                let auto_update_task = self
                    .auto_update_task_id
                    .and_then(|id| self.tasks.iter().find(|t| t.id == id));
                views::about_view::view(
                    &self.app_icon_handle,
                    &self.author_avatar_handle,
                    &self.update_status,
                    auto_update_task,
                    self.manager.data_dir(),
                    palette,
                )
            }
            nav => {
                let query = self.quick_input.trim().to_ascii_lowercase();
                let filter_text = !query.is_empty()
                    && !query.starts_with("http://")
                    && !query.starts_with("https://")
                    && !query.starts_with("magnet:?")
                    && !query.starts_with("ed2k://");

                let filtered: Vec<&DownloadTask> = self
                    .tasks
                    .iter()
                    .filter(|t| match nav {
                        NavSection::All => true,
                        NavSection::Downloading => t.status.is_active(),
                        NavSection::Completed => t.status == TaskStatus::Completed,
                        NavSection::Paused => matches!(
                            t.status,
                            TaskStatus::Paused
                                | TaskStatus::Queued
                                | TaskStatus::Error
                                | TaskStatus::LinkExpired
                        ),
                        NavSection::Category(cat) => t.category == cat,
                        _ => true,
                    })
                    .filter(|t| {
                        if !filter_text {
                            true
                        } else {
                            t.filename.to_ascii_lowercase().contains(&query)
                                || t.domain().to_ascii_lowercase().contains(&query)
                        }
                    })
                    .collect();

                let list_pane = views::task_list::view(
                    nav,
                    &filtered,
                    self.selected_task_id,
                    self.config.list_density,
                    &self.file_icons,
                    self.clipboard_banner_url.as_deref(),
                    self.toast.as_deref(),
                    palette,
                    strings,
                );

                let selected_task = self
                    .selected_task_id
                    .and_then(|id| self.tasks.iter().find(|t| t.id == id));

                if let Some(task) = selected_task {
                    let icon_key = Self::task_icon_key(task);
                    let sys_icon = self
                        .file_icons
                        .get(&icon_key)
                        .or_else(|| self.file_icons.get(&icon_key.fallback_extension()))
                        .map(|(_, h)| h);
                    row![
                        list_pane,
                        rule::vertical(1).style(hairline_rule_style(palette)),
                        views::inspector::view(
                            task,
                            sys_icon,
                            self.show_replace_url,
                            &self.replace_url_input,
                            palette,
                            strings,
                        ),
                    ]
                    .height(Length::Fill)
                    .into()
                } else {
                    list_pane
                }
            }
        };

        let shell = column![
            titlebar,
            rule::horizontal(1).style(hairline_rule_style(palette)),
            row![
                sidebar,
                rule::vertical(1).style(hairline_rule_style(palette)),
                center_and_inspector
            ]
            .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        let root_frame = container(shell)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(panel_style(
                palette.bg_window,
                Some(palette.hairline),
                0.0,
            ));

        let with_modal: Element<'_, Message> = if self.modal.open {
            let modal_layer = views::new_task::view(&self.modal, palette);
            stack![root_frame, modal_layer].into()
        } else {
            root_frame.into()
        };

        let with_delete: Element<'_, Message> = if let Some(dlg) = &self.delete_confirm {
            let delete_layer = views::delete_confirm::view(dlg, palette);
            stack![with_modal, delete_layer].into()
        } else {
            with_modal
        };

        let with_ext_install: Element<'_, Message> = if let Some(modal) = &self.ext_install_modal {
            let ext_layer =
                views::ext_install_modal::view(modal, self.browser_ext_connected, palette);
            stack![with_delete, ext_layer].into()
        } else {
            with_delete
        };

        let with_drop: Element<'_, Message> = if self.drag_hovering {
            let drop_card = container(
                column![
                    icon(IconKind::Link, 32.0, palette.accent),
                    text("松开即可导入链接到 Siphon")
                        .size(16)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text("支持浏览器超链接、地址栏 URL、HLS .m3u8、Magnet 磁力、.torrent 种子、eD2k 电驴与 .url 快捷方式")
                        .size(13)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                ]
                .spacing(8)
                .align_x(Alignment::Center),
            )
            .padding([28, 40])
            .style(panel_style(palette.bg_modal, Some(palette.accent), 14.0));

            let overlay = container(drop_card)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(panel_style(palette.bg_overlay, None, 0.0));

            stack![with_ext_install, overlay].into()
        } else {
            with_ext_install
        };

        if let Some(menu) = self.context_menu
            && !matches!(
                menu.target,
                InputTarget::CaptureFilename
                    | InputTarget::CaptureSaveDir
                    | InputTarget::SniffPageUrl
                    | InputTarget::SniffSearch
                    | InputTarget::SniffSaveDir
            )
        {
            let menu_layer = self.view_context_menu(menu, palette);
            stack![with_drop, menu_layer].into()
        } else {
            with_drop
        }
    }
}

fn runtime_event_stream() -> impl iced::futures::Stream<Item = Message> {
    stream::channel(128, async |mut output| {
        let _ = init_shared_runtime();
        let Some(shared) = SHARED.get() else {
            return;
        };

        let mgr = Arc::clone(&shared.manager);
        let mut engine_rx = mgr.subscribe();
        let mut tray_rx = shared.tray_rx.lock().expect("tray_rx lock").take();
        let mut clip_rx = shared.clip_rx.lock().expect("clip_rx lock").take();
        let mut hook_rx = shared.hook_rx.lock().expect("hook_rx lock").take();
        let mut drop_rx = shared.drop_rx.lock().expect("drop_rx lock").take();

        // Start Named Pipe IPC server & local HTTP browser capture bridge inside Iced's Tokio runtime
        let (ipc_tx, mut ipc_rx) = mpsc::unbounded_channel();
        spawn_ipc_server(ipc_tx.clone());
        let init_cfg = mgr.config().await;
        spawn_browser_http_bridge(
            init_cfg.browser_bridge_port,
            Arc::clone(&shared.browser_enabled),
            ipc_tx,
        );

        // Pre-warm shared BitTorrent session + persisted DHT routing table in the background so
        // subsequent Magnet / .torrent tasks join swarms in milliseconds instead of cold-booting.
        let bt_warm = Arc::clone(&shared.bt_engine);
        tokio::spawn(async move {
            bt_warm
                .warmup(
                    &init_cfg.bt,
                    &init_cfg.default_save_dir,
                    init_cfg.proxy_url.as_deref(),
                )
                .await;
        });

        // Start 500ms global speed ticker and 10s scheduler ticker inside Iced's Tokio runtime
        let mut speed_ticker = tokio::time::interval(std::time::Duration::from_millis(500));
        let mut sched_ticker = tokio::time::interval(std::time::Duration::from_secs(10));

        loop {
            tokio::select! {
                res = engine_rx.recv() => {
                    match res {
                        Ok(ev) => {
                            let _ = output.send(Message::EngineEvent(ev)).await;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            // Re-sync all tasks if a high-frequency progress burst lagged the broadcast ring
                            for t in mgr.list_tasks().await {
                                let _ = output.send(Message::EngineEvent(TaskEvent::TaskUpdated(t))).await;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                Some(ipc_cmd) = ipc_rx.recv() => {
                    let _ = output.send(Message::IpcEvent(ipc_cmd)).await;
                }
                Some(tray_act) = async {
                    match tray_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    let _ = output.send(Message::TrayEvent(tray_act)).await;
                }
                Some(clip_url) = async {
                    match clip_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    let _ = output.send(Message::ClipboardUrlDetected(clip_url)).await;
                }
                Some(hook_ev) = async {
                    match hook_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    let _ = output.send(Message::WinHook(hook_ev)).await;
                }
                Some(drop_ev) = async {
                    match drop_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    let _ = output.send(Message::DropEvent(drop_ev)).await;
                }
                _ = speed_ticker.tick() => {
                    let tasks = mgr.list_tasks().await;
                    let mut total_speed_bps = 0u64;
                    let mut active_count = 0usize;
                    for t in &tasks {
                        if t.status.is_active() {
                            active_count += 1;
                            total_speed_bps = total_speed_bps.saturating_add(t.speed_bps);
                        }
                    }
                    let _ = output.send(Message::EngineEvent(TaskEvent::GlobalSpeedTick {
                        total_speed_bps,
                        active_count,
                    })).await;
                }
                _ = sched_ticker.tick() => {
                    let _ = output.send(Message::SchedulerTick).await;
                }
            }
        }
    })
}

/// Recursively copy a directory (used for one-click external plugin folder import).
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

/// Extract an HTTP/HTTPS/Magnet/eD2k URL from a dragged `.url` Windows shortcut or text file.
fn extract_url_from_dropped_file(path: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("URL=").or_else(|| trimmed.strip_prefix("url=")) {
            let candidate = rest.trim();
            let low = candidate.to_ascii_lowercase();
            if low.starts_with("http://")
                || low.starts_with("https://")
                || low.starts_with("magnet:?")
                || low.starts_with("ed2k://|file|")
            {
                return Some(candidate.to_string());
            }
        }
        let low = trimmed.to_ascii_lowercase();
        if low.starts_with("http://")
            || low.starts_with("https://")
            || low.starts_with("magnet:?")
            || low.starts_with("ed2k://|file|")
        {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub fn run_gui(start_minimized: bool) -> iced::Result {
    let _ = init_shared_runtime();
    if let Some(shared) = SHARED.get() {
        shared
            .start_minimized
            .store(start_minimized, Ordering::Relaxed);
    }

    iced::daemon(SiphonApp::boot, SiphonApp::update, SiphonApp::view)
        .title(SiphonApp::title)
        .theme(SiphonApp::theme)
        .style(SiphonApp::app_style)
        .subscription(SiphonApp::subscription)
        .font(include_bytes!("../assets/fonts/MiSans-Medium.otf").as_slice())
        .font(include_bytes!("../assets/fonts/MiSans-Demibold.otf").as_slice())
        .font(include_bytes!("../assets/fonts/MiSans-Semibold.otf").as_slice())
        .default_font(FONT_REGULAR)
        .antialiasing(true)
        .run()
}
