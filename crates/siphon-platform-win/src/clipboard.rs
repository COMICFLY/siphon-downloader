use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

/// Read plain text from the Windows clipboard cleanly.
pub fn read_clipboard_text() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let text = cb.get_text().ok()?;
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Write plain text to the Windows clipboard cleanly.
pub fn write_clipboard_text(text: &str) -> bool {
    let Ok(mut cb) = arboard::Clipboard::new() else {
        return false;
    };
    cb.set_text(text.to_string()).is_ok()
}

/// Heuristically determine whether a clipboard snippet is a direct downloadable HTTP/HTTPS URL.
pub fn extract_downloadable_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 || trimmed.contains('\n') || trimmed.contains(' ') {
        return None;
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let path = parsed.path().to_ascii_lowercase();
    let known_exts = [
        ".zip", ".7z", ".rar", ".tar", ".gz", ".xz", ".bz2", ".zst", ".tgz", ".iso", ".img",
        ".vhdx", ".exe", ".msi", ".msix", ".dmg", ".deb", ".rpm", ".apk", ".mp4", ".mkv", ".webm",
        ".mov", ".mp3", ".flac", ".pdf", ".epub", ".bin", ".pkg", ".whl", ".jar",
    ];
    let looks_like_download = known_exts.iter().any(|ext| path.ends_with(ext))
        || path.contains("/download")
        || path.contains("/releases/download/");

    if looks_like_download {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Spawn a zero-polling Win32 `AddClipboardFormatListener` event loop on a lightweight thread.
#[cfg(windows)]
pub fn spawn_clipboard_monitor(
    enabled: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<String>,
) {
    std::thread::Builder::new()
        .name("siphon-clipboard".into())
        .spawn(move || {
            run_win32_clipboard_loop(enabled, tx);
        })
        .ok();
}

#[cfg(not(windows))]
pub fn spawn_clipboard_monitor(_enabled: Arc<AtomicBool>, _tx: mpsc::UnboundedSender<String>) {}

#[cfg(windows)]
fn run_win32_clipboard_loop(enabled: Arc<AtomicBool>, tx: mpsc::UnboundedSender<String>) {
    use std::cell::RefCell;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::{
        AddClipboardFormatListener, RemoveClipboardFormatListener,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
        RegisterClassW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE,
        WNDCLASSW,
    };
    use windows::core::w;

    thread_local! {
        static CTX: RefCell<Option<(Arc<AtomicBool>, mpsc::UnboundedSender<String>, String)>> =
            const { RefCell::new(None) };
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_CLIPBOARDUPDATE {
            CTX.with(|cell| {
                if let Some((enabled, tx, last_seen)) = cell.borrow_mut().as_mut()
                    && enabled.load(Ordering::Relaxed)
                    && let Ok(mut cb) = arboard::Clipboard::new()
                    && let Ok(text) = cb.get_text()
                    && let Some(url) = extract_downloadable_url(&text)
                    && *last_seen != url
                {
                    *last_seen = url.clone();
                    let _ = tx.send(url);
                }
            });
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    CTX.with(|cell| {
        *cell.borrow_mut() = Some((enabled, tx, String::new()));
    });

    unsafe {
        let Ok(hinstance) = GetModuleHandleW(None) else {
            return;
        };
        let class_name = w!("SiphonClipboardListenerWnd");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("SiphonClipboard"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance.into()),
            None,
        ) else {
            return;
        };

        if AddClipboardFormatListener(hwnd).is_err() {
            return;
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = RemoveClipboardFormatListener(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_downloadable_urls_and_ignores_plain_text() {
        assert_eq!(
            extract_downloadable_url("https://mirrors.tuna.tsinghua.edu.cn/ubuntu-24.04.iso"),
            Some("https://mirrors.tuna.tsinghua.edu.cn/ubuntu-24.04.iso".to_string())
        );
        assert_eq!(
            extract_downloadable_url("https://github.com/org/repo/releases/download/v1.0/app"),
            Some("https://github.com/org/repo/releases/download/v1.0/app".to_string())
        );
        assert_eq!(extract_downloadable_url("https://example.com/about"), None);
        assert_eq!(extract_downloadable_url("hello world"), None);
    }
}
