//! Window handling: creating the windows, the Mica background, showing,
//! sizing and focusing.

use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder};

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

/// Shows the quick search box centred on the current screen. The global
/// hotkey that calls this arrives in M4; until then the main window does.
///
/// The size is set on every show: a window created hidden keeps the size it
/// got at 100% scale, so on a scaled display (or after moving to another
/// monitor) it would come up too small and cut off the list.
pub fn show_quick(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("quick") {
        w.set_size(tauri::LogicalSize::new(QUICK_SIZE.0, QUICK_SIZE.1))?;
        w.center()?;
        w.show()?;
        w.set_focus()?;
        w.emit("quick-shown", ())?;
    }
    Ok(())
}

pub fn hide_quick(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("quick") {
        w.hide()?;
    }
    Ok(())
}
