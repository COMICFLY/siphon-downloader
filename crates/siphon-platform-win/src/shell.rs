use std::path::{Path, PathBuf};
use std::process::Command;

/// Normalize any mixed `/` and `\` separators into clean Windows `\` path components.
pub fn normalize_win_path(path: &Path) -> PathBuf {
    let raw = path
        .to_string_lossy()
        .replace('/', &std::path::MAIN_SEPARATOR.to_string());
    PathBuf::from(raw).components().collect()
}

/// Open Windows Explorer and highlight the target file (or open its parent folder if absent).
pub fn reveal_in_explorer(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let norm = normalize_win_path(path);
        if norm.exists() {
            let _ = Command::new("explorer.exe")
                .raw_arg(format!("/select,\"{}\"", norm.display()))
                .spawn();
        } else if let Some(parent) = norm.parent() {
            let _ = std::fs::create_dir_all(parent);
            let _ = Command::new("explorer.exe").arg(parent).spawn();
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

pub fn open_folder(dir: &Path) {
    #[cfg(windows)]
    {
        let norm = normalize_win_path(dir);
        let _ = std::fs::create_dir_all(&norm);
        let _ = Command::new("explorer.exe").arg(&norm).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
    }
}

pub fn open_file(path: &Path) {
    #[cfg(windows)]
    {
        let norm = normalize_win_path(path);
        if norm.exists() {
            let _ = Command::new("cmd")
                .args(["/C", "start", "", &norm.to_string_lossy()])
                .spawn();
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

pub fn open_url(url: &str) {
    let trimmed = url.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return;
    }
    #[cfg(windows)]
    {
        let _ = Command::new("cmd")
            .args(["/C", "start", "", trimmed])
            .spawn();
    }
}

#[cfg(windows)]
pub fn is_autostart_enabled() -> bool {
    read_hkcu_string(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        "Siphon",
    )
    .is_some_and(|v| !v.trim().is_empty())
}

#[cfg(not(windows))]
pub fn is_autostart_enabled() -> bool {
    false
}

#[cfg(windows)]
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
    };
    use windows::core::w;

    unsafe {
        let mut hkey = HKEY::default();
        let res = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        );
        if res.is_err() {
            return Err(std::io::Error::other("无法打开 Windows 自启注册表键"));
        }

        let op_res = if enabled {
            let exe = std::env::current_exe()?;
            let exe_clean = exe
                .to_string_lossy()
                .strip_prefix(r"\\?\")
                .map(|s| s.to_string())
                .unwrap_or_else(|| exe.to_string_lossy().to_string());
            let cmd = format!("\"{exe_clean}\" --minimized");
            let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
            RegSetValueExW(hkey, w!("Siphon"), None, REG_SZ, Some(bytes))
        } else {
            RegDeleteValueW(hkey, w!("Siphon"))
        };

        let _ = RegCloseKey(hkey);

        if enabled && op_res.is_err() {
            return Err(std::io::Error::other("写入开机自启注册表失败"));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set_autostart(_enabled: bool) -> std::io::Result<()> {
    Ok(())
}

/// Ensure a Win32 HWND is restored and brought to foreground (used when waking from `--minimized` tray startup).
#[cfg(windows)]
pub fn show_window_native(hwnd: u64) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SW_RESTORE, SetForegroundWindow, ShowWindow};
    unsafe {
        let h = HWND(hwnd as usize as *mut std::ffi::c_void);
        let _ = ShowWindow(h, SW_RESTORE);
        let _ = SetForegroundWindow(h);
    }
}

#[cfg(not(windows))]
pub fn show_window_native(_hwnd: u64) {}

const SIPHON_ICON_48_RGBA: &[u8] = include_bytes!("../assets/siphon-icon-48.rgba");

/// Return the 48x48 Siphon emblem RGBA bitmap synced 1:1 with the EXE icon and browser extension icon.
pub fn generate_siphon_icon_rgba() -> (Vec<u8>, u32, u32) {
    (SIPHON_ICON_48_RGBA.to_vec(), 48, 48)
}

const EXT_MANIFEST_JSON: &str = include_str!("../../../browser-extension/manifest.json");
const EXT_BACKGROUND_JS: &str = include_str!("../../../browser-extension/background.js");
const EXT_POPUP_HTML: &str = include_str!("../../../browser-extension/popup.html");
const EXT_POPUP_JS: &str = include_str!("../../../browser-extension/popup.js");
const EXT_ICON_16_PNG: &[u8] = include_bytes!("../../../browser-extension/icons/icon-16.png");
const EXT_ICON_32_PNG: &[u8] = include_bytes!("../../../browser-extension/icons/icon-32.png");
const EXT_ICON_48_PNG: &[u8] = include_bytes!("../../../browser-extension/icons/icon-48.png");
const EXT_ICON_128_PNG: &[u8] = include_bytes!("../../../browser-extension/icons/icon-128.png");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedBrowser {
    pub id: String,
    pub name: String,
    pub exe_path: PathBuf,
    pub extensions_url: String,
    pub is_default: bool,
}

#[cfg(windows)]
fn read_hkcu_string(subkey: &str, value_name: &str) -> Option<String> {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE, RegCloseKey, RegOpenKeyExW,
        RegQueryValueExW,
    };
    use windows::core::PCWSTR;

    let wide_key: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_val: Vec<u16> = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide_key.as_ptr()),
            None,
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return None;
        }
        let mut reg_type = REG_VALUE_TYPE::default();
        let mut byte_len = 0u32;
        let query_ok = RegQueryValueExW(
            hkey,
            PCWSTR(wide_val.as_ptr()),
            None,
            Some(&mut reg_type),
            None,
            Some(&mut byte_len),
        )
        .is_ok();
        if !query_ok || byte_len < 2 {
            let _ = RegCloseKey(hkey);
            return None;
        }
        let mut buf = vec![0u8; byte_len as usize];
        let read_ok = RegQueryValueExW(
            hkey,
            PCWSTR(wide_val.as_ptr()),
            None,
            Some(&mut reg_type),
            Some(buf.as_mut_ptr()),
            Some(&mut byte_len),
        )
        .is_ok();
        let _ = RegCloseKey(hkey);
        if !read_ok {
            return None;
        }
        let u16s: Vec<u16> = buf[..byte_len as usize]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&v| v != 0)
            .collect();
        Some(String::from_utf16_lossy(&u16s))
    }
}

/// Detect installed Chromium-based browsers on Windows and mark the system default browser.
pub fn detect_installed_browsers() -> Vec<DetectedBrowser> {
    let mut found = Vec::new();
    #[cfg(windows)]
    {
        let default_prog = read_hkcu_string(
            r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
            "ProgId",
        )
        .unwrap_or_default()
        .to_ascii_lowercase();

        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        let pf86 = std::env::var("ProgramFiles(x86)")
            .unwrap_or_else(|_| r"C:\Program Files (x86)".to_string());
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();

        let specs: &[(&str, &str, &str, &[&str], &[String])] = &[
            (
                "edge",
                "Microsoft Edge",
                "edge://extensions",
                &["msedge"],
                &[
                    format!(r"{pf86}\Microsoft\Edge\Application\msedge.exe"),
                    format!(r"{pf}\Microsoft\Edge\Application\msedge.exe"),
                    format!(r"{local}\Microsoft\Edge\Application\msedge.exe"),
                ],
            ),
            (
                "chrome",
                "Google Chrome",
                "chrome://extensions",
                &["chromehtml", "chrome"],
                &[
                    format!(r"{pf}\Google\Chrome\Application\chrome.exe"),
                    format!(r"{pf86}\Google\Chrome\Application\chrome.exe"),
                    format!(r"{local}\Google\Chrome\Application\chrome.exe"),
                ],
            ),
            (
                "brave",
                "Brave",
                "brave://extensions",
                &["brave"],
                &[
                    format!(r"{pf}\BraveSoftware\Brave-Browser\Application\brave.exe"),
                    format!(r"{local}\BraveSoftware\Brave-Browser\Application\brave.exe"),
                ],
            ),
            (
                "vivaldi",
                "Vivaldi",
                "vivaldi://extensions",
                &["vivaldi"],
                &[
                    format!(r"{local}\Vivaldi\Application\vivaldi.exe"),
                    format!(r"{pf}\Vivaldi\Application\vivaldi.exe"),
                ],
            ),
        ];

        for (id, name, ext_url, prog_hints, candidates) in specs {
            if let Some(exe) = candidates
                .iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
            {
                let is_default = prog_hints.iter().any(|h| default_prog.contains(h));
                found.push(DetectedBrowser {
                    id: (*id).to_string(),
                    name: (*name).to_string(),
                    exe_path: exe,
                    extensions_url: (*ext_url).to_string(),
                    is_default,
                });
            }
        }

        // Sort default browser first
        found.sort_by_key(|b| !b.is_default);
    }
    found
}

/// Deploy/update the built-in Chrome/Edge MV3 extension (including PNG icons) to
/// `%APPDATA%\Siphon\browser-extension` and register Native Messaging Hosts in HKCU.
pub fn deploy_browser_extension(data_dir: &Path) -> std::io::Result<PathBuf> {
    let ext_dir = normalize_win_path(&data_dir.join("browser-extension"));
    let icons_dir = ext_dir.join("icons");
    std::fs::create_dir_all(&icons_dir)?;
    std::fs::write(ext_dir.join("manifest.json"), EXT_MANIFEST_JSON)?;
    std::fs::write(ext_dir.join("background.js"), EXT_BACKGROUND_JS)?;
    std::fs::write(ext_dir.join("popup.html"), EXT_POPUP_HTML)?;
    std::fs::write(ext_dir.join("popup.js"), EXT_POPUP_JS)?;
    std::fs::write(icons_dir.join("icon-16.png"), EXT_ICON_16_PNG)?;
    std::fs::write(icons_dir.join("icon-32.png"), EXT_ICON_32_PNG)?;
    std::fs::write(icons_dir.join("icon-48.png"), EXT_ICON_48_PNG)?;
    std::fs::write(icons_dir.join("icon-128.png"), EXT_ICON_128_PNG)?;

    // Register Native Messaging host manifest in HKCU for Edge, Chrome, Brave & Vivaldi
    let host_dir = normalize_win_path(&data_dir.join("native-host"));
    let _ = std::fs::create_dir_all(&host_dir);
    let host_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("siphon-native-host.exe")))
        .unwrap_or_else(|| PathBuf::from("siphon-native-host.exe"));
    let host_manifest_path = host_dir.join("com.siphon.downloader.json");
    let escaped_exe = host_exe.to_string_lossy().replace('\\', "\\\\");
    let host_manifest = format!(
        r#"{{
  "name": "com.siphon.downloader",
  "description": "Siphon Native Messaging Host",
  "path": "{escaped_exe}",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://*/"
  ]
}}"#
    );
    let _ = std::fs::write(&host_manifest_path, host_manifest);

    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegSetValueExW,
        };
        use windows::core::PCWSTR;

        let manifest_str = host_manifest_path.to_string_lossy().to_string();
        let wide_val: Vec<u16> = manifest_str
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let val_bytes =
            unsafe { std::slice::from_raw_parts(wide_val.as_ptr().cast::<u8>(), wide_val.len() * 2) };

        for reg_path in [
            r"Software\Microsoft\Edge\NativeMessagingHosts\com.siphon.downloader",
            r"Software\Google\Chrome\NativeMessagingHosts\com.siphon.downloader",
            r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.siphon.downloader",
            r"Software\Vivaldi\NativeMessagingHosts\com.siphon.downloader",
        ] {
            let wide_key: Vec<u16> = reg_path.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let mut hkey = HKEY::default();
                if RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(wide_key.as_ptr()),
                    None,
                    None,
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &mut hkey,
                    None,
                )
                .is_ok()
                {
                    let _ = RegSetValueExW(hkey, None, None, REG_SZ, Some(val_bytes));
                    let _ = RegCloseKey(hkey);
                }
            }
        }
    }

    Ok(ext_dir)
}

/// Launch the chosen browser directly to its extensions page, copy the extension directory path
/// to clipboard, and highlight the `browser-extension` folder in Explorer for 1-drag installation.
pub fn launch_browser_extensions_page(browser: Option<&DetectedBrowser>, ext_dir: &Path) {
    let norm_dir = normalize_win_path(ext_dir);
    let _ = crate::clipboard::write_clipboard_text(&norm_dir.to_string_lossy());
    reveal_in_explorer(&norm_dir);

    #[cfg(windows)]
    {
        if let Some(b) = browser {
            let _ = Command::new(&b.exe_path).arg(&b.extensions_url).spawn();
        } else {
            let _ = Command::new("cmd")
                .args(["/C", "start", "", "msedge", "edge://extensions"])
                .spawn();
        }
    }
}

/// Backwards-compatible one-step wrapper that deploys the extension and opens the default browser.
pub fn install_browser_extension(data_dir: &Path) -> std::io::Result<PathBuf> {
    let ext_dir = deploy_browser_extension(data_dir)?;
    let browsers = detect_installed_browsers();
    launch_browser_extensions_page(browsers.first(), &ext_dir);
    Ok(ext_dir)
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Export the built-in browser extension as a standalone `.zip` archive (stored ZIP with CRC32)
/// so users can unpack or drag it into any Chromium browser independently.
pub fn export_browser_extension_zip(dest_zip: &Path) -> std::io::Result<PathBuf> {
    let norm = normalize_win_path(dest_zip);
    if let Some(parent) = norm.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let entries: &[(&str, &[u8])] = &[
        ("siphon-browser-extension/manifest.json", EXT_MANIFEST_JSON.as_bytes()),
        ("siphon-browser-extension/background.js", EXT_BACKGROUND_JS.as_bytes()),
        ("siphon-browser-extension/popup.html", EXT_POPUP_HTML.as_bytes()),
        ("siphon-browser-extension/popup.js", EXT_POPUP_JS.as_bytes()),
        ("siphon-browser-extension/icons/icon-16.png", EXT_ICON_16_PNG),
        ("siphon-browser-extension/icons/icon-32.png", EXT_ICON_32_PNG),
        ("siphon-browser-extension/icons/icon-48.png", EXT_ICON_48_PNG),
        ("siphon-browser-extension/icons/icon-128.png", EXT_ICON_128_PNG),
    ];

    let mut out = Vec::with_capacity(32 * 1024);
    let mut central = Vec::new();

    for (name, data) in entries {
        let name_bytes = name.as_bytes();
        let crc = crc32_ieee(data);
        let size = data.len() as u32;
        let local_offset = out.len() as u32;

        // Local file header
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 flag
        out.extend_from_slice(&0u16.to_le_bytes()); // method = store
        out.extend_from_slice(&0u16.to_le_bytes()); // mod time
        out.extend_from_slice(&0x5821u16.to_le_bytes()); // mod date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);

        // Central directory header
        central.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 flag
        central.extend_from_slice(&0u16.to_le_bytes()); // method = store
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x5821u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra len
        central.extend_from_slice(&0u16.to_le_bytes()); // comment len
        central.extend_from_slice(&0u16.to_le_bytes()); // disk start
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }

    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);

    // End of central directory record
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    std::fs::write(&norm, out)?;
    Ok(norm)
}
