//! Window handling: creating the windows, the Mica background, showing,
//! sizing and focusing.

use std::sync::Mutex;

use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
};

/// The window that was in front when the quick search box appeared: focus
/// goes back to it when the box closes, and auto-type (M5) types into it.
/// `None` when it was one of Mikoshi's own windows.
#[derive(Default)]
pub struct PreviousWindow(Mutex<Option<isize>>);

/// Mica needs Windows 11 (build 22000+). On Windows 10 the windows stay
/// transparent-capable but the frontend paints a solid background instead.
pub fn supports_mica() -> bool {
    windows_version::OsVersion::current().build >= 22000
}

/// Creates the windows described in tauri.conf.json. Called from `setup`
/// after the app state is registered; see the note there.
pub fn create_all(app: &AppHandle) -> tauri::Result<()> {
    for config in &app.config().app.windows {
        let mut builder = WebviewWindowBuilder::from_config(app, config)?;
        // The quick search box draws its own panel (rounded, with a shadow)
        // on a transparent window, so only the main window gets Mica.
        if config.label == "main" && supports_mica() {
            builder = builder.effects(EffectsBuilder::new().effect(Effect::Mica).build());
        }
        builder.build()?;
    }
    Ok(())
}

pub fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The main window starts hidden and the frontend shows it once it has
/// rendered, so it never flashes unstyled. If the frontend fails to load, the
/// window would stay hidden for good and leave an invisible process behind:
/// show it anyway after a few seconds.
pub fn show_main_if_frontend_is_stuck(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if let Some(w) = app.get_webview_window("main")
            && !w.is_visible().unwrap_or(true)
        {
            log::warn!("the frontend did not show the main window; showing it anyway");
            let _ = w.show();
        }
    });
}

/// Size of the quick search window, in logical pixels. The panel inside is
/// smaller; the rest is transparent margin for its shadow.
const QUICK_SIZE: (f64, f64) = (660.0, 520.0);

/// Shows the quick search box centred on the current screen, from the
/// global hotkey, the tray or the main window.
///
/// The size is set on every show: a window created hidden keeps the size it
/// got at 100% scale, so on a scaled display (or after moving to another
/// monitor) it would come up too small and cut off the list.
pub fn show_quick(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("quick") {
        if !w.is_visible().unwrap_or(false) {
            remember_foreground(app);
        }
        w.set_size(tauri::LogicalSize::new(QUICK_SIZE.0, QUICK_SIZE.1))?;
        w.center()?;
        w.show()?;
        w.set_focus()?;
        w.emit("quick-shown", ())?;
    }
    Ok(())
}

/// Hides the quick search box. With `restore`, the window that was in front
/// before gets the focus back; not when the box closes because the user
/// clicked somewhere else, which already chose where the focus goes.
pub fn hide_quick(app: &AppHandle, restore: bool) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("quick") {
        if restore && w.is_visible().unwrap_or(false) {
            // while the box still has the focus: Windows only lets the
            // foreground program hand the focus on
            focus_previous(app);
        }
        w.hide()?;
    }
    Ok(())
}

/// For the hotkey: a second press closes the box.
pub fn toggle_quick(app: &AppHandle) {
    let shown = app
        .get_webview_window("quick")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    let result = if shown {
        hide_quick(app, true)
    } else {
        show_quick(app)
    };
    if let Err(e) = result {
        log::error!("cannot show or hide quick search: {e}");
    }
}

fn is_ours(hwnd: HWND) -> bool {
    let mut pid = 0;
    // SAFETY: plain query; an invalid handle leaves `pid` at 0
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: no preconditions
    pid == unsafe { GetCurrentProcessId() }
}

fn remember_foreground(app: &AppHandle) {
    // SAFETY: no preconditions
    let hwnd = unsafe { GetForegroundWindow() };
    let previous = (!hwnd.is_invalid() && !is_ours(hwnd)).then_some(hwnd.0 as isize);
    if let Ok(mut p) = app.state::<PreviousWindow>().0.lock() {
        *p = previous;
    }
}

fn focus_previous(app: &AppHandle) {
    let Some(h) = app.state::<PreviousWindow>().0.lock().ok().and_then(|p| *p) else {
        return;
    };
    let hwnd = HWND(h as *mut core::ffi::c_void);
    // SAFETY: IsWindow accepts any value; the window may have closed since
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}
