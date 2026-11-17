//! Automatic locking (features.md §3.2): after a period without keyboard or
//! mouse input anywhere on the system, when Windows locks (Win+L) and when
//! the computer goes to sleep or hibernates. Minimizing is handled with the
//! window events in lib.rs.

use std::sync::OnceLock;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PBT_APMSUSPEND,
    RegisterClassW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_POWERBROADCAST,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_SESSION_LOCK,
};
use windows::core::w;

use crate::AppState;
use crate::session::lock_app;

/// How often the idle time is checked.
const POLL: Duration = Duration::from_secs(2);

/// The window procedure has no context argument, so the watcher finds the
/// app here.
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn start(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let idle_app = app.clone();
    std::thread::Builder::new()
        .name("idle-lock".into())
        .spawn(move || watch_idle(idle_app))
        .ok();
    std::thread::Builder::new()
        .name("session-watch".into())
        .spawn(watch_session)
        .ok();
}

/// Milliseconds since the last keyboard or mouse input in this session.
fn idle_ms() -> u32 {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a valid LASTINPUTINFO with cbSize set
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0;
    }
    // both are 32-bit tick counts that wrap after ~49 days
    // SAFETY: no preconditions
    unsafe { GetTickCount() }.wrapping_sub(info.dwTime)
}

fn watch_idle(app: AppHandle) {
    loop {
        std::thread::sleep(POLL);
        let state = app.state::<AppState>();
        let minutes = state
            .settings
            .lock()
            .map(|s| s.current.idle_lock_minutes)
            .unwrap_or(0);
        if minutes == 0 {
            continue;
        }
        let open = state
            .vault
            .lock()
            .map(|s| s.open.is_some())
            .unwrap_or(false);
        if open && u64::from(idle_ms()) >= u64::from(minutes) * 60_000 {
            lock_app(&app, "idle");
        }
    }
}

unsafe extern "system" fn session_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let reason = match msg {
        WM_WTSSESSION_CHANGE if wparam.0 as u32 == WTS_SESSION_LOCK => Some("session locked"),
        WM_POWERBROADCAST if wparam.0 as u32 == PBT_APMSUSPEND => Some("going to sleep"),
        _ => None,
    };
    if let (Some(reason), Some(app)) = (reason, APP.get()) {
        lock_app(app, reason);
    }
    // SAFETY: forwarding the message we were given
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A hidden top-level window that receives session changes (Win+L) and power
/// broadcasts (sleep). A message-only window would not get the broadcasts.
fn watch_session() {
    // SAFETY: standard window class registration and message loop; the
    // window lives as long as this thread, which lives as long as the app
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            log::warn!("auto-lock on Win+L and sleep is not available (no module handle)");
            return;
        };
        let class = w!("MikoshiSessionWatch");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(session_proc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            log::warn!("auto-lock on Win+L and sleep is not available (window class)");
            return;
        }
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Mikoshi"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            log::warn!("auto-lock on Win+L and sleep is not available (window)");
            return;
        };
        if WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).is_err() {
            log::warn!("auto-lock on Win+L is not available");
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
