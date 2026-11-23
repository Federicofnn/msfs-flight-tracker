//! Tray icon next to the clock.
//!
//! Closing the window hides the tracker from the taskbar instead of quitting, so a
//! flight in progress is never lost; the icon in the notification area brings the
//! window back or ends the program for good.

use std::sync::Mutex;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Direct window calls: while the window is hidden Windows stops sending paint
/// messages, so the tracker would stop updating (and stop recording).
mod win {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsWindowVisible, PostMessageW, SetForegroundWindow, ShowWindow, SW_RESTORE,
        SW_SHOW, WM_PAINT,
    };

    fn title() -> Vec<u16> {
        "MSFS Flight Tracker\0".encode_utf16().collect()
    }

    pub fn handle() -> Option<HWND> {
        let hwnd = unsafe { FindWindowW(std::ptr::null(), title().as_ptr()) };
        if hwnd.is_null() {
            None
        } else {
            Some(hwnd)
        }
    }

    pub fn hidden(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd) == 0 }
    }

    /// A paint message is what makes winit run one frame, hidden or not.
    pub fn tick(hwnd: HWND) {
        unsafe { PostMessageW(hwnd, WM_PAINT, 0, 0) };
    }

    pub fn reveal(hwnd: HWND) {
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
}

/// Keeps the frames coming while the window is out of the way, so a flight in
/// progress is still recorded and the tray menu is still honoured.
fn spawn_pump() {
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let Some(hwnd) = win::handle() else {
            continue;
        };
        if win::hidden(hwnd) {
            win::tick(hwnd);
        }
    });
}

/// What the pilot asked from the tray.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayMsg {
    Show,
    Quit,
}

static PENDING: Mutex<Vec<TrayMsg>> = Mutex::new(Vec::new());
static CTX: Mutex<Option<eframe::egui::Context>> = Mutex::new(None);

fn push(msg: TrayMsg) {
    if let Ok(mut pending) = PENDING.lock() {
        pending.push(msg);
    }
    // The window is hidden while the message arrives, so wake the interface up.
    if let Ok(ctx) = CTX.lock() {
        if let Some(ctx) = ctx.as_ref() {
            ctx.request_repaint();
        }
    }
}

/// The messages collected since the last frame.
pub fn take() -> Vec<TrayMsg> {
    PENDING
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}

fn icon() -> Option<tray_icon::Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/icon_png/icon_32.png")).ok()?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    tray_icon::Icon::from_rgba(rgba.into_raw(), width, height).ok()
}

/// Creates the icon. On Windows the tray needs the thread that runs the window
/// event loop, so this is called from the first frame, not before the loop starts.
pub fn start(ctx: &eframe::egui::Context) -> Option<TrayIcon> {
    if let Ok(mut slot) = CTX.lock() {
        *slot = Some(ctx.clone());
    }
    let menu = Menu::new();
    let open = MenuItem::new("Apri MSFS Flight Tracker", true, None);
    let quit = MenuItem::new("Chiudi MSFS Flight Tracker", true, None);
    menu.append(&open).ok()?;
    menu.append(&quit).ok()?;

    let open_id = open.id().clone();
    let quit_id = quit.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            push(TrayMsg::Quit);
        } else if event.id == open_id {
            if let Some(hwnd) = win::handle() {
                win::reveal(hwnd);
            }
            push(TrayMsg::Show);
        }
    }));
    TrayIconEvent::set_event_handler(Some(|event: TrayIconEvent| {
        match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
        | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => {
            if let Some(hwnd) = win::handle() {
                win::reveal(hwnd);
            }
            push(TrayMsg::Show)
        }
        _ => {}
        }
    }));

    // Windows drops the click position when the icon sits in the hidden-icons
    // flyout, so the menu (which always works) is what reopens the window.
    let tray = TrayIconBuilder::new()
        .with_menu_on_left_click(true)
        .with_menu(Box::new(menu))
        .with_tooltip("MSFS Flight Tracker — click to open")
        .with_icon(icon()?)
        .build()
        .ok();
    if tray.is_some() {
        spawn_pump();
    }
    tray
}
