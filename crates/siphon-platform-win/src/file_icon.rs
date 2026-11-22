use std::path::{Path, PathBuf};

/// Cache key identifying a Windows Shell file icon.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileIconKey {
    /// Completed file on disk that may embed its own custom icon (e.g. `.exe`, `.msi`, `.ico`).
    RealFile(PathBuf),
    /// Lowercase extension including leading dot (e.g. `".zip"`, `".7z"`, `".iso"`), or `""` when extensionless.
    Extension(String),
}

impl FileIconKey {
    /// Whether this lowercase extension (with leading dot) can contain a per-file embedded icon.
    pub fn supports_embedded_icon(ext: &str) -> bool {
        matches!(
            ext,
            ".exe" | ".msi" | ".ico" | ".lnk" | ".scr" | ".cur" | ".ani"
        )
    }

    /// Derive the Windows Shell icon key for a download task without doing filesystem I/O in `view()`.
    /// - Completed executables/installers/icons map to `RealFile(final_path)` so they resolve their
    ///   own embedded application icon once downloaded.
    /// - All other tasks map to `Extension(ext)` so they follow Windows' live file-type association.
    pub fn for_task(filename: &str, final_path: &Path, is_completed: bool) -> Self {
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_ascii_lowercase()))
            .unwrap_or_default();

        if is_completed && Self::supports_embedded_icon(&ext) {
            Self::RealFile(final_path.to_path_buf())
        } else {
            Self::Extension(ext)
        }
    }

    pub fn fallback_extension(&self) -> Self {
        match self {
            Self::RealFile(path) => {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| format!(".{}", e.to_ascii_lowercase()))
                    .unwrap_or_default();
                Self::Extension(ext)
            }
            Self::Extension(ext) => Self::Extension(ext.clone()),
        }
    }

    pub fn is_real_file(&self) -> bool {
        matches!(self, Self::RealFile(_))
    }
}

/// Extracted RGBA8 bitmap from Windows Shell / PE resource with a fast change-detection hash.
#[derive(Debug, Clone)]
pub struct FileIconBitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub hash: u64,
}

fn fnv1a_64(width: u32, height: u32, rgba: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in width.to_le_bytes().into_iter().chain(height.to_le_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    for &b in rgba {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(windows)]
fn dispatch_sta_icon_batch(
    keys: Vec<FileIconKey>,
) -> tokio::sync::oneshot::Receiver<Vec<(FileIconKey, Option<FileIconBitmap>)>> {
    use std::sync::{OnceLock, mpsc as std_mpsc};
    use tokio::sync::oneshot;

    type BatchResult = Vec<(FileIconKey, Option<FileIconBitmap>)>;
    struct IconRequest {
        keys: Vec<FileIconKey>,
        reply: oneshot::Sender<BatchResult>,
    }

    static STA_WORKER: OnceLock<std_mpsc::Sender<IconRequest>> = OnceLock::new();

    let tx = STA_WORKER.get_or_init(|| {
        let (tx, rx) = std_mpsc::channel::<IconRequest>();
        let _ = std::thread::Builder::new()
            .name("siphon-icon-sta".to_string())
            .spawn(move || {
                use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
                let ole_ok = unsafe { OleInitialize(None) }.is_ok();
                while let Ok(req) = rx.recv() {
                    let out = req
                        .keys
                        .into_iter()
                        .map(|k| {
                            let bmp = extract_file_icon_on_sta(&k);
                            (k, bmp)
                        })
                        .collect();
                    let _ = req.reply.send(out);
                }
                if ole_ok {
                    unsafe { OleUninitialize() };
                }
            });
        tx
    });

    let (reply_tx, reply_rx) = oneshot::channel();
    let _ = tx.send(IconRequest {
        keys,
        reply: reply_tx,
    });
    reply_rx
}

/// Extract a batch of Windows system file icons on a **single dedicated, persistent COM STA worker thread**.
///
/// Serializing all `SHGetFileInfoW` / `ExtractIconExW` calls onto one `OleInitialize` STA thread
/// eliminates BOTH:
/// 1. Tokio blocking-pool COM apartment mismatches (`RPC_E_CHANGED_MODE`), and
/// 2. Windows `shell32.dll` concurrency races when multiple threads call `SHGetFileInfoW` at once.
pub async fn extract_file_icons_async(
    keys: Vec<FileIconKey>,
) -> Vec<(FileIconKey, Option<FileIconBitmap>)> {
    if keys.is_empty() {
        return Vec::new();
    }

    #[cfg(windows)]
    {
        dispatch_sta_icon_batch(keys).await.unwrap_or_default()
    }

    #[cfg(not(windows))]
    {
        keys.into_iter().map(|k| (k, None)).collect()
    }
}

/// Synchronously extract a Windows Shell / PE icon for `key` via the serialized STA worker thread.
#[cfg(windows)]
pub fn extract_file_icon_rgba(key: &FileIconKey) -> Option<FileIconBitmap> {
    dispatch_sta_icon_batch(vec![key.clone()])
        .blocking_recv()
        .ok()
        .and_then(|mut v| v.pop())
        .and_then(|(_, bmp)| bmp)
}

#[cfg(not(windows))]
pub fn extract_file_icon_rgba(_key: &FileIconKey) -> Option<FileIconBitmap> {
    None
}

#[cfg(windows)]
fn extract_file_icon_on_sta(key: &FileIconKey) -> Option<FileIconBitmap> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
        GetDIBits, GetObjectW, HDC, HGDIOBJ, ReleaseDC,
    };
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES};
    use windows::Win32::UI::Shell::{
        ExtractIconExW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES,
        SHGetFileInfoW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
    use windows::core::PCWSTR;

    struct IconGuard(HICON);
    impl Drop for IconGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                let _ = unsafe { DestroyIcon(self.0) };
            }
        }
    }

    struct GdiObjGuard(HGDIOBJ);
    impl Drop for GdiObjGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                let _ = unsafe { DeleteObject(self.0) };
            }
        }
    }

    struct ScreenDcGuard(HDC);
    impl Drop for ScreenDcGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                let _ = unsafe { ReleaseDC(Some(HWND::default()), self.0) };
            }
        }
    }

    let icon_guard: IconGuard = match key {
        FileIconKey::RealFile(path) => {
            if !path.is_file() {
                return None;
            }
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();

            // 1) For PE binaries / icon files (.exe, .ico, .scr), extract directly via ExtractIconExW first!
            //    This reads RT_GROUP_ICON straight from the file without waiting on Windows Shell's
            //    asynchronous icon cache, and works immediately after download rename.
            let mut direct_icon: Option<IconGuard> = None;
            if matches!(ext.as_str(), "exe" | "ico" | "scr" | "dll") {
                for attempt in 0..3 {
                    let mut large = HICON::default();
                    let count = unsafe {
                        ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut large), None, 1)
                    };
                    if count > 0 && count != u32::MAX && !large.is_invalid() {
                        direct_icon = Some(IconGuard(large));
                        break;
                    }
                    // If Windows Defender briefly holds an exclusive scan lock right after rename,
                    // wait briefly once before retrying.
                    if attempt < 2 && std::fs::File::open(path).is_err() {
                        std::thread::sleep(std::time::Duration::from_millis(60));
                    } else {
                        break;
                    }
                }
            }

            // 2) Fall back to SHGetFileInfoW on the real file (handles .msi, .lnk, .cur, .ani,
            //    and CLI .exe files that have no embedded icon resource).
            //    IMPORTANT: Never substitute Extension(".exe") here if the real file lookup fails,
            //    so we never poison the RealFile cache entry with a stale generic icon.
            if let Some(guard) = direct_icon {
                guard
            } else {
                let mut shfi = SHFILEINFOW::default();
                let ret = unsafe {
                    SHGetFileInfoW(
                        PCWSTR(wide.as_ptr()),
                        FILE_FLAGS_AND_ATTRIBUTES(0),
                        Some(&mut shfi),
                        size_of::<SHFILEINFOW>() as u32,
                        SHGFI_ICON | SHGFI_LARGEICON,
                    )
                };
                if ret == 0 || shfi.hIcon.is_invalid() {
                    return None;
                }
                IconGuard(shfi.hIcon)
            }
        }
        FileIconKey::Extension(ext) => {
            let synthetic = if ext.is_empty() {
                "siphon_file".to_string()
            } else {
                format!("siphon{ext}")
            };
            let wide: Vec<u16> = synthetic.encode_utf16().chain(std::iter::once(0)).collect();
            let mut shfi = SHFILEINFOW::default();
            let ret = unsafe {
                SHGetFileInfoW(
                    PCWSTR(wide.as_ptr()),
                    FILE_ATTRIBUTE_NORMAL,
                    Some(&mut shfi),
                    size_of::<SHFILEINFOW>() as u32,
                    SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
                )
            };
            if ret == 0 || shfi.hIcon.is_invalid() {
                return None;
            }
            IconGuard(shfi.hIcon)
        }
    };

    let mut icon_info = ICONINFO::default();
    if unsafe { GetIconInfo(icon_guard.0, &mut icon_info) }.is_err() {
        return None;
    }
    let _color_guard = GdiObjGuard(HGDIOBJ(icon_info.hbmColor.0));
    let _mask_guard = GdiObjGuard(HGDIOBJ(icon_info.hbmMask.0));

    let hdc = ScreenDcGuard(unsafe { GetDC(Some(HWND::default())) });
    if hdc.0.is_invalid() {
        return None;
    }

    if !icon_info.hbmColor.is_invalid() {
        let mut bmp = BITMAP::default();
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(icon_info.hbmColor.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bmp as *mut BITMAP as *mut c_void),
            )
        };
        if got == 0 || bmp.bmWidth <= 0 || bmp.bmHeight <= 0 {
            return None;
        }
        let width = bmp.bmWidth as u32;
        let height = bmp.bmHeight as u32;
        let pixel_count = (width as usize).checked_mul(height as usize)?;
        let mut bgra = vec![0u8; pixel_count * 4];

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // top-down scanlines
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let scanlines = unsafe {
            GetDIBits(
                hdc.0,
                icon_info.hbmColor,
                0,
                height,
                Some(bgra.as_mut_ptr() as *mut c_void),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        if scanlines == 0 {
            return None;
        }

        let has_alpha = bgra.chunks_exact(4).any(|px| px[3] != 0);
        let mut rgba = vec![0u8; pixel_count * 4];

        if has_alpha {
            for (src, dst) in bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = src[3];
            }
        } else {
            // Legacy 24-bit icon without alpha channel: reconstruct transparency from hbmMask
            let mut mask = vec![0u8; pixel_count * 4];
            let mut mask_ok = false;
            if !icon_info.hbmMask.is_invalid() {
                let mut mask_bmi = bmi;
                let ms = unsafe {
                    GetDIBits(
                        hdc.0,
                        icon_info.hbmMask,
                        0,
                        height,
                        Some(mask.as_mut_ptr() as *mut c_void),
                        &mut mask_bmi,
                        DIB_RGB_COLORS,
                    )
                };
                mask_ok = ms != 0;
            }
            for i in 0..pixel_count {
                let off = i * 4;
                let transparent = mask_ok && mask[off] != 0;
                rgba[off] = bgra[off + 2];
                rgba[off + 1] = bgra[off + 1];
                rgba[off + 2] = bgra[off];
                rgba[off + 3] = if transparent { 0 } else { 255 };
            }
        }

        let hash = fnv1a_64(width, height, &rgba);
        Some(FileIconBitmap {
            width,
            height,
            rgba,
            hash,
        })
    } else if !icon_info.hbmMask.is_invalid() {
        // Pure monochrome icon (top half = AND mask, bottom half = XOR mask)
        let mut bmp = BITMAP::default();
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(icon_info.hbmMask.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bmp as *mut BITMAP as *mut c_void),
            )
        };
        if got == 0 || bmp.bmWidth <= 0 || bmp.bmHeight < 2 {
            return None;
        }
        let width = bmp.bmWidth as u32;
        let full_h = bmp.bmHeight as u32;
        let height = full_h / 2;
        let mut raw = vec![0u8; (width as usize) * (full_h as usize) * 4];
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(full_h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let scanlines = unsafe {
            GetDIBits(
                hdc.0,
                icon_info.hbmMask,
                0,
                full_h,
                Some(raw.as_mut_ptr() as *mut c_void),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        if scanlines == 0 {
            return None;
        }
        let half_bytes = (width as usize) * (height as usize) * 4;
        let and_plane = &raw[..half_bytes];
        let xor_plane = &raw[half_bytes..half_bytes * 2];
        let mut rgba = vec![0u8; half_bytes];
        for i in 0..(width as usize * height as usize) {
            let off = i * 4;
            let and_on = and_plane[off] != 0;
            let xor_on = xor_plane[off] != 0;
            if and_on && !xor_on {
                // transparent
            } else {
                let v = if xor_on { 255 } else { 0 };
                rgba[off] = v;
                rgba[off + 1] = v;
                rgba[off + 2] = v;
                rgba[off + 3] = 255;
            }
        }
        let hash = fnv1a_64(width, height, &rgba);
        Some(FileIconBitmap {
            width,
            height,
            rgba,
            hash,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_system_file_type_icons_with_valid_alpha() {
        for ext in [".zip", ".7z", ".rar", ".iso", ".exe", ".pdf", ".mp4", ".txt", ""] {
            let key = FileIconKey::Extension(ext.to_string());
            let bmp = extract_file_icon_rgba(&key)
                .unwrap_or_else(|| panic!("expected Windows system icon for '{ext}'"));
            assert!(bmp.width >= 16 && bmp.height >= 16);
            assert_eq!(bmp.rgba.len(), (bmp.width * bmp.height * 4) as usize);
            assert!(
                bmp.rgba.chunks_exact(4).any(|px| px[3] > 0),
                "icon for '{ext}' should have visible non-transparent pixels"
            );
        }
    }

    #[tokio::test]
    async fn extracts_embedded_exe_icon_immediately_after_rename_on_sta_worker() {
        let generic_exe = extract_file_icons_async(vec![FileIconKey::Extension(".exe".into())])
            .await
            .into_iter()
            .next()
            .and_then(|(_, b)| b)
            .expect("generic .exe icon");

        // Simulate a downloaded software binary renamed from .siphon.part -> .exe
        let dir = std::env::temp_dir().join(format!("siphon-icon-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let part_path = dir.join("sample_app.exe.siphon.part");
        let final_path = dir.join("sample_app.exe");
        let src = Path::new(r"C:\Windows\explorer.exe");
        assert!(src.is_file());
        std::fs::copy(src, &part_path).expect("copy sample exe to .part");
        std::fs::rename(&part_path, &final_path).expect("finalize .part -> .exe");

        let key = FileIconKey::for_task("sample_app.exe", &final_path, true);
        assert!(key.is_real_file());
        let extracted = extract_file_icons_async(vec![key])
            .await
            .into_iter()
            .next()
            .and_then(|(_, b)| b)
            .expect("embedded icon from freshly renamed .exe");

        assert!(extracted.width >= 16 && extracted.height >= 16);
        assert_ne!(
            extracted.hash, generic_exe.hash,
            "freshly renamed .exe must yield its real embedded app icon, not the generic .exe icon"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
