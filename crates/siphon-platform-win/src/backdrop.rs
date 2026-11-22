#[cfg(windows)]
use std::ffi::c_void;

/// Apply Windows 11 DWM backdrop (Mica or Solid), rounded corners, and dark/light frame sync.
#[cfg(windows)]
pub fn apply_win11_backdrop(hwnd_raw: u64, use_mica: bool, dark: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE, DWMWA_SYSTEMBACKDROP_TYPE,
        DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
    };
    use windows::Win32::UI::Controls::MARGINS;

    if hwnd_raw == 0 {
        return;
    }

    let hwnd = HWND(hwnd_raw as usize as *mut c_void);

    unsafe {
        // 1) Native Windows 11 rounded corners + shadow even on borderless window
        let corner_pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corner_pref as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );

        // 2) Sync immersive dark mode flag
        let dark_val: i32 = if dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark_val as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );

        // 3) Extend client frame for Mica transparency, or reset margins for solid surface
        let margins = if use_mica {
            MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            }
        } else {
            MARGINS {
                cxLeftWidth: 0,
                cxRightWidth: 0,
                cyTopHeight: 0,
                cyBottomHeight: 0,
            }
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        // 4) Request Windows 11 Mica backdrop (DWMSBT_MAINWINDOW = 2, DWMSBT_NONE = 1)
        let backdrop = DWM_SYSTEMBACKDROP_TYPE(if use_mica { 2 } else { 1 });
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&backdrop as *const DWM_SYSTEMBACKDROP_TYPE).cast(),
            std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        );
    }
}

#[cfg(not(windows))]
pub fn apply_win11_backdrop(_hwnd_raw: u64, _use_mica: bool, _dark: bool) {}

/// Detect whether Windows 11 currently uses Dark Mode for apps.
#[cfg(windows)]
pub fn is_system_dark_mode() -> bool {
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW,
    };
    use windows::core::w;

    let mut data: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        )
    };
    if status.is_ok() {
        data == 0
    } else {
        true
    }
}

#[cfg(not(windows))]
pub fn is_system_dark_mode() -> bool {
    true
}
