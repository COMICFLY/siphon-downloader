#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod i18n;
mod icons;
mod state;
mod theme;
mod views;
mod widgets;

use tracing_subscriber::EnvFilter;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .without_time()
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let start_minimized = args
        .iter()
        .any(|a| a == "--minimized" || a.eq_ignore_ascii_case("siphon://minimized"));

    // Support `--data-dir <PATH>` or `--data-dir=<PATH>` for controllable data directory
    for (i, arg) in args.iter().enumerate() {
        if let Some(val) = arg.strip_prefix("--data-dir=") {
            if !val.trim().is_empty() {
                siphon_core::AppConfig::set_data_dir_override(std::path::PathBuf::from(val.trim()));
            }
        } else if arg == "--data-dir"
            && let Some(val) = args.get(i + 1)
            && !val.trim().is_empty()
        {
            siphon_core::AppConfig::set_data_dir_override(std::path::PathBuf::from(val.trim()));
        }
    }

    let initial_url = args
        .iter()
        .find(|a| {
            let low = a.to_ascii_lowercase();
            low.starts_with("http://")
                || low.starts_with("https://")
                || low.starts_with("magnet:?")
                || low.starts_with("ed2k://|file|")
        })
        .cloned();

    // Single-instance check: if Siphon is already running, hand off via Named Pipe IPC and exit.
    if siphon_platform_win::try_notify_existing_instance(initial_url) {
        return Ok(());
    }

    app::run_gui(start_minimized)
}
