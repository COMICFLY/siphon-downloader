use crate::shell::generate_siphon_icon_rgba;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    ShowWindow,
    NewTask,
    PauseAll,
    ResumeAll,
    Quit,
}

#[derive(Clone)]
pub struct TrayController {
    tooltip_tx: std::sync::mpsc::Sender<String>,
}

impl TrayController {
    pub fn set_tooltip(&self, text: impl Into<String>) {
        let _ = self.tooltip_tx.send(text.into());
    }
}

/// Spawn the Windows system tray icon on a dedicated Win32 event loop thread.
#[cfg(windows)]
pub fn spawn_system_tray(action_tx: mpsc::UnboundedSender<TrayAction>) -> TrayController {
    let (tooltip_tx, tooltip_rx) = std::sync::mpsc::channel::<String>();

    std::thread::Builder::new()
        .name("siphon-tray".into())
        .spawn(move || {
            run_win32_tray_thread(action_tx, tooltip_rx);
        })
        .ok();

    TrayController { tooltip_tx }
}

#[cfg(not(windows))]
pub fn spawn_system_tray(_action_tx: mpsc::UnboundedSender<TrayAction>) -> TrayController {
    let (tooltip_tx, _) = std::sync::mpsc::channel();
    TrayController { tooltip_tx }
}

#[cfg(windows)]
fn run_win32_tray_thread(
    action_tx: mpsc::UnboundedSender<TrayAction>,
    tooltip_rx: std::sync::mpsc::Receiver<String>,
) {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };

    let (rgba, w, h) = generate_siphon_icon_rgba();
    let Ok(icon) = Icon::from_rgba(rgba, w, h) else {
        return;
    };

    let menu = Menu::new();
    let item_show = MenuItem::new("显示 Siphon", true, None);
    let item_new = MenuItem::new("新建下载", true, None);
    let item_pause = MenuItem::new("暂停全部", true, None);
    let item_resume = MenuItem::new("恢复全部", true, None);
    let item_quit = MenuItem::new("退出 Siphon", true, None);

    let _ = menu.append(&item_show);
    let _ = menu.append(&item_new);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&item_pause);
    let _ = menu.append(&item_resume);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&item_quit);

    let id_show = item_show.id().clone();
    let id_new = item_new.id().clone();
    let id_pause = item_pause.id().clone();
    let id_resume = item_resume.id().clone();
    let id_quit = item_quit.id().clone();

    let Ok(tray) = TrayIconBuilder::new()
        .with_tooltip("Siphon — 待机中")
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .build()
    else {
        return;
    };

    let tray_rx = TrayIconEvent::receiver();
    let menu_rx = MenuEvent::receiver();

    let mut msg = MSG::default();
    loop {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        while let Ok(ev) = tray_rx.try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                let _ = action_tx.send(TrayAction::ShowWindow);
            }
        }

        while let Ok(ev) = menu_rx.try_recv() {
            let act = if ev.id == id_show {
                Some(TrayAction::ShowWindow)
            } else if ev.id == id_new {
                Some(TrayAction::NewTask)
            } else if ev.id == id_pause {
                Some(TrayAction::PauseAll)
            } else if ev.id == id_resume {
                Some(TrayAction::ResumeAll)
            } else if ev.id == id_quit {
                Some(TrayAction::Quit)
            } else {
                None
            };
            if let Some(a) = act {
                let quit = a == TrayAction::Quit;
                let _ = action_tx.send(a);
                if quit {
                    return;
                }
            }
        }

        // Apply only the latest pending tooltip update
        let mut latest_tip = None;
        while let Ok(tip) = tooltip_rx.try_recv() {
            latest_tip = Some(tip);
        }
        if let Some(tip) = latest_tip {
            let _ = tray.set_tooltip(Some(tip));
        }

        std::thread::sleep(std::time::Duration::from_millis(35));
    }
}
