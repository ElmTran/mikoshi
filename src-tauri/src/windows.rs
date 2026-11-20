//! Window handling: creating the windows, the Mica background, showing,
//! hiding, sizing and focusing.
//!
//! A hidden window's WebView is put to sleep (`set_awake`), which gives most
//! of its memory back. So every show and hide goes through `show` and `hide`
//! here, never straight to the window.

use std::sync::Mutex;

use tauri::webview::PageLoadEvent;
use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WebviewWindowBuilder};
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
        // dev-desktop mode (see `dev_desktop`): the WebView must not grab the
        // keyboard focus when it is made, which would activate the window
        #[cfg(debug_assertions)]
        if dev_desktop::wanted().is_some() {
            builder = builder.focused(false);
        }
        builder.on_page_load(sleep_if_left_hidden).build()?;
    }
    #[cfg(debug_assertions)]
    dev_desktop::apply(app);
    Ok(())
}

/// Development only: Mikoshi runs on another virtual desktop for a test (see
/// `dev_desktop`). Then nothing may reach the desktop someone works on: no
/// window is activated, no global hotkey is registered, nothing is typed.
/// Always `false` in release builds.
pub fn dev_quiet() -> bool {
    #[cfg(debug_assertions)]
    return dev_desktop::wanted().is_some();
    #[cfg(not(debug_assertions))]
    false
}

/// Development only: keeps a test run off the desktop someone is working on.
///
/// With `MIKOSHI_DEV_DESKTOP` set to a virtual desktop's GUID (find one with
/// `tools/desktop-of.ps1`), Mikoshi moves its windows to that desktop as soon
/// as they exist. Windows only lets a program move its own windows, so this
/// has to happen in here. Pair it with the WebView2 flags in architecture.md
/// ("不打扰桌面的测试") so the page keeps drawing on a desktop nobody looks at.
/// Not compiled into release builds.
#[cfg(debug_assertions)]
mod dev_desktop {
    use tauri::{AppHandle, Manager};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
    use windows::core::GUID;

    pub const VAR: &str = "MIKOSHI_DEV_DESKTOP";

    /// `{xxxxxxxx-xxxx-...}` with or without braces.
    fn parse(text: &str) -> Option<GUID> {
        let hex: String = text
            .chars()
            .filter(|c| !matches!(c, '{' | '}' | '-') && !c.is_whitespace())
            .collect();
        (hex.len() == 32)
            .then(|| u128::from_str_radix(&hex, 16).ok())
            .flatten()
            .map(GUID::from_u128)
    }

    pub fn wanted() -> Option<GUID> {
        let text = std::env::var(VAR).ok()?;
        let guid = parse(&text);
        if guid.is_none() {
            log::warn!("{VAR} is not a GUID; windows stay where Windows puts them");
        }
        guid
    }

    pub fn apply(app: &AppHandle) {
        if wanted().is_none() {
            return;
        }
        for label in ["main", "quick"] {
            let Some(w) = app.get_webview_window(label) else {
                continue;
            };
            // A window without a taskbar button belongs to no virtual desktop
            // (Windows shows it on whichever one is in front), so moving it
            // does nothing. The quick search box gets a button while testing.
            // That takes effect a little later, so `show` moves it again.
            if label == "quick" {
                let _ = w.set_skip_taskbar(false);
            }
            move_window(&w);
        }
    }

    /// Puts a window on the desktop in `MIKOSHI_DEV_DESKTOP`. Harmless when
    /// it is there already.
    pub fn move_window(w: &tauri::WebviewWindow) {
        let Some(desktop) = wanted() else { return };
        let Ok(hwnd) = w.hwnd() else { return };
        let hwnd = HWND(hwnd.0);
        // SAFETY: the window handle belongs to this process and is alive;
        // the COM calls run on this (the main) thread
        let result = unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL)
                .and_then(|m| m.MoveWindowToDesktop(hwnd, &desktop))
        };
        if let Err(e) = result {
            log::warn!("{}: cannot move to the desktop in {VAR}: {e}", w.label());
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn reads_guids() {
            let g = super::parse("0be3f803-fe19-44ff-99af-3dbcf861013b").unwrap();
            assert_eq!(g.to_u128(), 0x0be3f803_fe19_44ff_99af_3dbcf861013b);
            assert_eq!(
                super::parse("{0BE3F803-FE19-44FF-99AF-3DBCF861013B}"),
                Some(g)
            );
            assert_eq!(super::parse("nope"), None);
        }
    }
}

/// The windows are created hidden. The quick search box stays hidden until
/// asked for, and so does the main window when started with Windows: once
/// the page has loaded and set itself up, put it to sleep. (The main window
/// otherwise shows itself when it has rendered.)
fn sleep_if_left_hidden(w: WebviewWindow, payload: tauri::webview::PageLoadPayload<'_>) {
    if payload.event() != PageLoadEvent::Finished {
        return;
    }
    let app = w.app_handle();
    let stays_hidden = w.label() == "quick" || app.state::<crate::AppState>().start_hidden;
    if !stays_hidden {
        return;
    }
    // "Finished" is the document load; give the page a moment to run its
    // first commands and subscribe to the events it needs
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(3));
        if !w.is_visible().unwrap_or(true) {
            set_awake(&w, false);
        }
    });
}

/// Shows a window, waking its WebView first. Wake before anything is sent to
/// the page: a suspended page does not run it.
pub fn show(w: &WebviewWindow) -> tauri::Result<()> {
    set_awake(w, true);
    #[cfg(debug_assertions)]
    dev_desktop::move_window(w);
    w.show()
}

/// Hides a window and puts its WebView to sleep.
pub fn hide(w: &WebviewWindow) -> tauri::Result<()> {
    w.hide()?;
    set_awake(w, false);
    Ok(())
}

/// Wakes a WebView or puts it to sleep. Asleep, it keeps the page (nothing
/// reloads) but runs no script and trims its memory; waking takes a few
/// milliseconds. Only for hidden windows: a visible WebView cannot sleep,
/// and showing one wakes it anyway. Failures are logged; the window still
/// works, it only keeps its memory.
#[cfg(windows)]
pub fn set_awake(w: &WebviewWindow, awake: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_3, ICoreWebView2_19,
    };
    use windows_core::Interface;

    let label = w.label().to_string();
    let result = w.with_webview(move |webview| {
        let controller = webview.controller();
        // SAFETY: COM calls on the controller Tauri gave us, made on the
        // thread that owns it (with_webview runs on the main thread)
        let result = unsafe {
            (|| -> windows_core::Result<()> {
                // first: a visible WebView cannot be suspended
                controller.SetIsVisible(awake)?;
                let core = controller.CoreWebView2()?;
                let level = if awake {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                } else {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                };
                core.cast::<ICoreWebView2_19>()?
                    .SetMemoryUsageTargetLevel(level)?;
                let core = core.cast::<ICoreWebView2_3>()?;
                if awake {
                    core.Resume()
                } else {
                    let label = label.clone();
                    let done = webview2_com::TrySuspendCompletedHandler::create(Box::new(
                        move |result, suspended| {
                            match result {
                                Ok(()) if suspended => log::debug!("{label}: WebView asleep"),
                                Ok(()) => log::debug!("{label}: WebView did not go to sleep"),
                                Err(e) => log::warn!("{label}: WebView cannot sleep: {e}"),
                            }
                            Ok(())
                        },
                    ));
                    core.TrySuspend(&done)
                }
            })()
        };
        if let Err(e) = result {
            let what = if awake { "wake" } else { "put to sleep" };
            log::warn!("{label}: cannot {what} the WebView: {e}");
        }
    });
    if let Err(e) = result {
        log::warn!("cannot reach the WebView of {}: {e}", w.label());
    }
}

#[cfg(not(windows))]
pub fn set_awake(_w: &WebviewWindow, _awake: bool) {}

pub fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = show(&w);
        if !dev_quiet() {
            let _ = w.set_focus();
        }
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
            let _ = show(&w);
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
    show_quick_mode(app, false)
}

/// What the quick search box shows when it opens.
#[derive(Clone, serde::Serialize)]
pub struct QuickShown {
    /// auto-type: list the entries that fit the window in front (several
    /// matched, or the database was locked); the user picks one
    pub pick: bool,
}

pub fn show_quick_mode(app: &AppHandle, pick: bool) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("quick") {
        if !w.is_visible().unwrap_or(false) {
            remember_foreground(app);
        }
        w.set_size(tauri::LogicalSize::new(QUICK_SIZE.0, QUICK_SIZE.1))?;
        w.center()?;
        show(&w)?;
        if !dev_quiet() {
            w.set_focus()?;
        }
        w.emit("quick-shown", QuickShown { pick })?;
    }
    Ok(())
}

/// Hides the quick search box. With `restore`, the window that was in front
/// before gets the focus back; not when the box closes because the user
/// clicked somewhere else, which already chose where the focus goes.
pub fn hide_quick(app: &AppHandle, restore: bool) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("quick") {
        let visible = w.is_visible().unwrap_or(false);
        // Hide first: Windows then activates the window underneath the usual
        // way, and it puts its keyboard focus back where it was. Activating
        // it while the box is still up left Windows 11 Notepad with the
        // focus on no control at all.
        hide(&w)?;
        if restore && visible {
            // in case Windows picked another window; we may still hand the
            // focus on, having been in front a moment ago
            focus_previous(app);
        }
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

pub fn is_ours(hwnd: HWND) -> bool {
    let mut pid = 0;
    // SAFETY: plain query; an invalid handle leaves `pid` at 0
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: no preconditions
    pid == unsafe { GetCurrentProcessId() }
}

fn remember_foreground(app: &AppHandle) {
    // SAFETY: no preconditions
    let hwnd = unsafe { GetForegroundWindow() };
    let previous =
        (!hwnd.is_invalid() && !is_ours(hwnd) && !dev_quiet()).then_some(hwnd.0 as isize);
    if let Ok(mut p) = app.state::<PreviousWindow>().0.lock() {
        *p = previous;
    }
}

/// The window to auto-type into (see `PreviousWindow`).
pub fn previous(app: &AppHandle) -> Option<isize> {
    app.state::<PreviousWindow>().0.lock().ok().and_then(|p| *p)
}

fn focus_previous(app: &AppHandle) {
    let Some(h) = app.state::<PreviousWindow>().0.lock().ok().and_then(|p| *p) else {
        return;
    };
    let hwnd = HWND(h as *mut core::ffi::c_void);
    // SAFETY: IsWindow accepts any value; the window may have closed since
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() && GetForegroundWindow() != hwnd {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}
