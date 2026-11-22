pub mod backdrop;
pub mod clipboard;
pub mod drop_target;
pub mod file_icon;
pub mod ipc;
pub mod shell;
pub mod tray;
pub mod win_hooks;

pub use backdrop::{apply_win11_backdrop, is_system_dark_mode};
pub use clipboard::{
    extract_downloadable_url, read_clipboard_text, spawn_clipboard_monitor, write_clipboard_text,
};
pub use drop_target::{DropEvent, extract_urls_from_text, install_url_drop_target};
pub use file_icon::{
    FileIconBitmap, FileIconKey, extract_file_icon_rgba, extract_file_icons_async,
};
pub use ipc::{
    BrowserCapturePayload, DEFAULT_BROWSER_BRIDGE_PORT, IpcCommand, SIPHON_PIPE_NAME,
    spawn_browser_http_bridge, spawn_ipc_server, try_notify_existing_instance,
};
pub use shell::{
    DetectedBrowser, deploy_browser_extension, detect_installed_browsers, execute_power_action,
    export_browser_extension_zip, generate_siphon_icon_rgba, install_browser_extension,
    is_autostart_enabled, launch_browser_extensions_page, launch_silent_installer_update,
    open_file, open_folder, open_url, reveal_in_explorer, set_autostart, show_window_native,
};
pub use tray::{TrayAction, TrayController, spawn_system_tray};
pub use win_hooks::{WinHookEvent, install_window_hooks};
