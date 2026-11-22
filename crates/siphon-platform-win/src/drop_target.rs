use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum DropEvent {
    Hovering(bool),
    DroppedUrls(Vec<String>),
    DroppedFiles(Vec<PathBuf>),
}

/// Extract all valid HTTP/HTTPS URLs from arbitrary dropped text (single URL or multi-line list).
pub fn extract_urls_from_text(raw: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for token in raw.split_whitespace() {
        let trimmed = token
            .trim()
            .trim_matches(|c| matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']'));
        let low = trimmed.to_ascii_lowercase();
        if (low.starts_with("http://")
            || low.starts_with("https://")
            || low.starts_with("magnet:?"))
            && url::Url::parse(trimmed).is_ok()
            && seen.insert(trimmed.to_string())
        {
            urls.push(trimmed.to_string());
        }
    }
    urls
}

/// Replace winit's file-only OLE IDropTarget with Siphon's full URL + Text + File IDropTarget.
///
/// MUST be invoked on the UI thread that owns `hwnd_raw` (e.g., inside `iced::window::run`).
#[cfg(windows)]
pub fn install_url_drop_target(hwnd_raw: u64, tx: mpsc::UnboundedSender<DropEvent>) {
    use std::cell::Cell;
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::{HWND, POINTL};
    use windows::Win32::System::Com::{
        DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL,
    };
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::{
        CF_HDROP, CF_TEXT, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK,
        DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop,
        ReleaseStgMedium, RevokeDragDrop,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows::core::{Ref, Result as WinResult, implement, w};

    if hwnd_raw == 0 {
        return;
    }

    let cf_url_w = unsafe { RegisterClipboardFormatW(w!("UniformResourceLocatorW")) } as u16;
    let cf_url_a = unsafe { RegisterClipboardFormatW(w!("UniformResourceLocator")) } as u16;

    fn make_fmt(cf: u16) -> FORMATETC {
        FORMATETC {
            cfFormat: cf,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    unsafe fn has_supported_format(data: &IDataObject, cf_url_w: u16, cf_url_a: u16) -> bool {
        let formats = [
            cf_url_w,
            cf_url_a,
            CF_UNICODETEXT.0,
            CF_TEXT.0,
            CF_HDROP.0,
        ];
        for cf in formats {
            if cf == 0 {
                continue;
            }
            let fmt = make_fmt(cf);
            if unsafe { data.QueryGetData(&fmt) }.is_ok() {
                return true;
            }
        }
        false
    }

    unsafe fn read_utf16_format(data: &IDataObject, cf: u16) -> Option<String> {
        if cf == 0 {
            return None;
        }
        let fmt = make_fmt(cf);
        let mut medium = unsafe { data.GetData(&fmt) }.ok()?;
        let result = (|| {
            let hglobal = unsafe { medium.u.hGlobal };
            let ptr = unsafe { GlobalLock(hglobal) } as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            unsafe {
                while len < 65_536 && *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                let s = String::from_utf16_lossy(slice);
                let _ = GlobalUnlock(hglobal);
                Some(s)
            }
        })();
        unsafe { ReleaseStgMedium(&mut medium) };
        result
    }

    unsafe fn read_ansi_format(data: &IDataObject, cf: u16) -> Option<String> {
        if cf == 0 {
            return None;
        }
        let fmt = make_fmt(cf);
        let mut medium = unsafe { data.GetData(&fmt) }.ok()?;
        let result = (|| {
            let hglobal = unsafe { medium.u.hGlobal };
            let ptr = unsafe { GlobalLock(hglobal) } as *const u8;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            unsafe {
                while len < 65_536 && *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                let s = String::from_utf8_lossy(slice).to_string();
                let _ = GlobalUnlock(hglobal);
                Some(s)
            }
        })();
        unsafe { ReleaseStgMedium(&mut medium) };
        result
    }

    unsafe fn read_hdrop_files(data: &IDataObject) -> Vec<PathBuf> {
        let fmt = make_fmt(CF_HDROP.0);
        let Ok(mut medium) = (unsafe { data.GetData(&fmt) }) else {
            return Vec::new();
        };
        let mut paths = Vec::new();
        let hdrop = HDROP(unsafe { medium.u.hGlobal.0 });
        let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
        for i in 0..count {
            let len = unsafe { DragQueryFileW(hdrop, i, None) } as usize;
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len + 1];
            let written = unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) } as usize;
            if written > 0 {
                paths.push(PathBuf::from(OsString::from_wide(&buf[..written])));
            }
        }
        unsafe { ReleaseStgMedium(&mut medium) };
        paths
    }

    #[implement(IDropTarget)]
    struct SiphonDropTarget {
        tx: mpsc::UnboundedSender<DropEvent>,
        cf_url_w: u16,
        cf_url_a: u16,
        valid_hover: Cell<bool>,
    }

    impl IDropTarget_Impl for SiphonDropTarget_Impl {
        fn DragEnter(
            &self,
            pdataobj: Ref<'_, IDataObject>,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> WinResult<()> {
            let valid = pdataobj
                .as_ref()
                .map(|d| unsafe { has_supported_format(d, self.cf_url_w, self.cf_url_a) })
                .unwrap_or(false);
            self.valid_hover.set(valid);
            if valid {
                let _ = self.tx.send(DropEvent::Hovering(true));
                if !pdweffect.is_null() {
                    unsafe {
                        *pdweffect = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_LINK.0);
                    }
                }
            } else if !pdweffect.is_null() {
                unsafe {
                    *pdweffect = DROPEFFECT_NONE;
                }
            }
            Ok(())
        }

        fn DragOver(
            &self,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> WinResult<()> {
            if !pdweffect.is_null() {
                unsafe {
                    *pdweffect = if self.valid_hover.get() {
                        DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_LINK.0)
                    } else {
                        DROPEFFECT_NONE
                    };
                }
            }
            Ok(())
        }

        fn DragLeave(&self) -> WinResult<()> {
            if self.valid_hover.replace(false) {
                let _ = self.tx.send(DropEvent::Hovering(false));
            }
            Ok(())
        }

        fn Drop(
            &self,
            pdataobj: Ref<'_, IDataObject>,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> WinResult<()> {
            self.valid_hover.set(false);
            let _ = self.tx.send(DropEvent::Hovering(false));

            if let Some(data) = pdataobj.as_ref() {
                // 1) Try browser UniformResourceLocatorW / UniformResourceLocator / Unicode text
                let mut urls = Vec::new();
                if let Some(raw) = unsafe { read_utf16_format(data, self.cf_url_w) } {
                    urls.extend(extract_urls_from_text(&raw));
                }
                if urls.is_empty()
                    && let Some(raw) = unsafe { read_utf16_format(data, CF_UNICODETEXT.0) }
                {
                    urls.extend(extract_urls_from_text(&raw));
                }
                if urls.is_empty()
                    && let Some(raw) = unsafe { read_ansi_format(data, self.cf_url_a) }
                {
                    urls.extend(extract_urls_from_text(&raw));
                }
                if urls.is_empty()
                    && let Some(raw) = unsafe { read_ansi_format(data, CF_TEXT.0) }
                {
                    urls.extend(extract_urls_from_text(&raw));
                }

                if !urls.is_empty() {
                    let _ = self.tx.send(DropEvent::DroppedUrls(urls));
                    if !pdweffect.is_null() {
                        unsafe { *pdweffect = DROPEFFECT_COPY };
                    }
                    return Ok(());
                }

                // 2) Fallback to dropped Explorer files (e.g. .url / .txt)
                let files = unsafe { read_hdrop_files(data) };
                if !files.is_empty() {
                    let _ = self.tx.send(DropEvent::DroppedFiles(files));
                    if !pdweffect.is_null() {
                        unsafe { *pdweffect = DROPEFFECT_COPY };
                    }
                    return Ok(());
                }
            }

            if !pdweffect.is_null() {
                unsafe { *pdweffect = DROPEFFECT_NONE };
            }
            Ok(())
        }
    }

    let hwnd = HWND(hwnd_raw as usize as *mut c_void);
    let target: IDropTarget = SiphonDropTarget {
        tx,
        cf_url_w,
        cf_url_a,
        valid_hover: Cell::new(false),
    }
    .into();

    unsafe {
        let _ = OleInitialize(None);
        let _ = RevokeDragDrop(hwnd);
        if let Err(e) = RegisterDragDrop(hwnd, &target) {
            tracing::warn!(error = %e, "注册 Siphon OLE URL 拖放处理器失败");
        }
    }
}

#[cfg(not(windows))]
pub fn install_url_drop_target(_hwnd_raw: u64, _tx: mpsc::UnboundedSender<DropEvent>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_single_and_multiple_urls_cleanly() {
        let single = extract_urls_from_text("https://example.com/file.zip");
        assert_eq!(single, vec!["https://example.com/file.zip"]);

        let multi = extract_urls_from_text(
            "Check <https://a.example.com/1.iso> and https://b.example.com/2.zip\nhttps://a.example.com/1.iso",
        );
        assert_eq!(
            multi,
            vec![
                "https://a.example.com/1.iso",
                "https://b.example.com/2.zip"
            ]
        );
    }
}
