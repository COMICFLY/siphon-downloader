use tokio::sync::mpsc;

/// Events intercepted directly at the Win32 HWND subclass layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinHookEvent {
    /// Synthetic `Win+V` paste, `WM_PASTE`, or `Shift+Insert`.
    Paste,
    /// Windows Shell file associations or icon cache changed (`SHCNE_ASSOCCHANGED` / `SHCNE_UPDATEIMAGE` / `WM_SETTINGCHANGE`).
    ShellIconsChanged,
}

/// Install a lightweight ComCtl32 window subclass on Siphon's HWND to catch:
/// 1. Windows 11 `Win + V` Clipboard History synthetic paste, `WM_PASTE`, and `Shift + Insert`
/// 2. Live Windows Shell file-association / icon-theme changes (`SHChangeNotifyRegister` + `WM_SETTINGCHANGE`)
#[cfg(windows)]
pub fn install_window_hooks(hwnd_raw: u64, tx: mpsc::UnboundedSender<WinHookEvent>) {
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_INSERT, VK_SHIFT,
    };
    use windows::Win32::UI::Shell::{
        DefSubclassProc, SHCNE_ASSOCCHANGED, SHCNE_UPDATEIMAGE, SHCNRF_InterruptLevel,
        SHCNRF_ShellLevel, SHChangeNotifyDeregister, SHChangeNotifyEntry, SHChangeNotifyRegister,
        SetWindowSubclass,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_APP, WM_KEYDOWN, WM_NCDESTROY, WM_PASTE, WM_SETFOCUS, WM_SETTINGCHANGE,
    };
    use windows_core::BOOL;

    const SUBCLASS_ID: usize = 0x51F0_5101;
    const WM_APP_SHELL_NOTIFY: u32 = WM_APP + 101;

    struct HookState {
        tx: Option<mpsc::UnboundedSender<WinHookEvent>>,
        last_focus: Option<Instant>,
        notify_id: u32,
        last_shell_notify: Option<Instant>,
    }
    static HOOK_STATE: OnceLock<Mutex<HookState>> = OnceLock::new();

    if hwnd_raw == 0 {
        return;
    }

    let slot = HOOK_STATE.get_or_init(|| {
        Mutex::new(HookState {
            tx: None,
            last_focus: None,
            notify_id: 0,
            last_shell_notify: None,
        })
    });

    let hwnd = HWND(hwnd_raw as usize as *mut c_void);

    {
        let mut st = slot.lock().expect("hook state lock");
        st.tx = Some(tx);
        if st.notify_id == 0 {
            let entry = SHChangeNotifyEntry {
                pidl: ptr::null_mut(),
                fRecursive: BOOL(1),
            };
            let events = (SHCNE_ASSOCCHANGED.0 | SHCNE_UPDATEIMAGE.0) as i32;
            st.notify_id = unsafe {
                SHChangeNotifyRegister(
                    hwnd,
                    SHCNRF_ShellLevel | SHCNRF_InterruptLevel,
                    events,
                    WM_APP_SHELL_NOTIFY,
                    1,
                    &entry,
                )
            };
        }
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uid: usize,
        _ref_data: usize,
    ) -> LRESULT {
        if msg == WM_SETFOCUS {
            if let Some(lock) = HOOK_STATE.get() {
                lock.lock().expect("hook state lock").last_focus = Some(Instant::now());
            }
            return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        }

        if msg == WM_APP_SHELL_NOTIFY || msg == WM_SETTINGCHANGE {
            if let Some(lock) = HOOK_STATE.get()
                && let Ok(mut st) = lock.lock()
            {
                let debounced = st
                    .last_shell_notify
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(250));
                if !debounced {
                    st.last_shell_notify = Some(Instant::now());
                    if let Some(tx) = st.tx.as_ref() {
                        let _ = tx.send(WinHookEvent::ShellIconsChanged);
                    }
                }
            }
            if msg == WM_APP_SHELL_NOTIFY {
                return LRESULT(0);
            }
            return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        }

        if msg == WM_NCDESTROY {
            if let Some(lock) = HOOK_STATE.get()
                && let Ok(mut st) = lock.lock()
                && st.notify_id != 0
            {
                let id = st.notify_id;
                st.notify_id = 0;
                let _ = unsafe { SHChangeNotifyDeregister(id) };
            }
            return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        }

        let should_paste = match msg {
            WM_PASTE => true,
            WM_KEYDOWN => {
                let vk = (wparam.0 & 0xFFFF) as u16;
                let scancode = ((lparam.0 as usize) >> 16) & 0xFF;
                let ctrl_down = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
                let shift_down = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;

                let just_focused = HOOK_STATE
                    .get()
                    .and_then(|l| l.lock().ok().and_then(|s| s.last_focus))
                    .is_some_and(|t| t.elapsed() <= Duration::from_millis(450));

                // Catch:
                // 1) Win+V synthetic Ctrl+V (scancode == 0 OR injected right after Win+V returns focus)
                // 2) Universal Shift+Insert paste
                (vk == b'V' as u16 && ctrl_down && (scancode == 0 || just_focused))
                    || (vk == VK_INSERT.0 && shift_down)
            }
            _ => false,
        };

        if should_paste {
            if let Some(lock) = HOOK_STATE.get()
                && let Some(tx) = lock.lock().expect("hook state lock").tx.as_ref()
            {
                let _ = tx.send(WinHookEvent::Paste);
            }
            return LRESULT(0);
        }

        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    let _ = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) };
}

#[cfg(not(windows))]
pub fn install_window_hooks(_hwnd_raw: u64, _tx: mpsc::UnboundedSender<WinHookEvent>) {}
