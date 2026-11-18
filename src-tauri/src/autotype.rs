//! Auto-type entry points (features.md §3.10): the global hotkey, the quick
//! search box and the main window. The typing itself is in typing.rs.

use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::browser;
use crate::error::AppError;
use crate::typing::{self, Plan, TypeError};
use crate::windows;

pub fn app_error(e: TypeError) -> AppError {
    match e {
        TypeError::BadSequence => AppError::AutotypeSequence,
        TypeError::MissingField => AppError::AutotypeField,
        TypeError::NoTotp => AppError::TotpInvalid,
        TypeError::NoWindow => AppError::AutotypeNoWindow,
        TypeError::Elevated => AppError::AutotypeElevated,
        TypeError::KeysHeld | TypeError::FocusLost | TypeError::Rejected => {
            AppError::AutotypeFailed
        }
    }
}

/// The global hotkey: the window in front decides. One fitting entry is
/// typed at once; several, or a locked database, open the quick search box
/// listing them; none opens it for searching.
pub fn on_hotkey(app: &AppHandle) {
    let Some(target) = typing::foreground_window() else {
        // Mikoshi itself is in front: nothing to type into
        return show(app, false);
    };
    // reading a browser's address can take a moment: not on the main thread
    let app = app.clone();
    std::thread::spawn(move || hotkey_for(&app, target));
}

fn hotkey_for(app: &AppHandle, target: isize) {
    let title = typing::window_title(target);
    let state = app.state::<AppState>();
    let locked = state.vault.lock().map(|s| s.open.is_none()).unwrap_or(true);
    if locked {
        // unlock in the box, then pick from the fitting entries
        return show(app, true);
    }
    let url = browser::page_url(target);
    let result = state.vault.lock().ok().and_then(|s| {
        let vault = s.open.as_ref()?;
        let mut matches = vault.autotype_matches(&title, url.as_deref());
        Some(if matches.len() == 1 {
            let (entry, sequence) = matches.remove(0);
            Ok(typing::plan(vault, &entry.id, &sequence, target))
        } else {
            Err(!matches.is_empty())
        })
    });
    match result {
        Some(Ok(Ok(plan))) => start(app, plan),
        Some(Ok(Err(e))) => failed(app, e),
        Some(Err(pick)) => show(app, pick),
        // locked in the meantime
        None => show(app, true),
    }
}

fn show(app: &AppHandle, pick: bool) {
    if let Err(e) = windows::show_quick_mode(app, pick) {
        log::error!("cannot show quick search: {e}");
    }
}

/// Types on a thread of its own; reports a failure afterwards.
pub fn start(app: &AppHandle, plan: Plan) {
    let delay = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|s| s.current.autotype_key_delay_ms)
        .unwrap_or(20);
    let app = app.clone();
    std::thread::Builder::new()
        .name("auto-type".into())
        .spawn(move || {
            if let Err(e) = typing::run(plan, delay) {
                failed(&app, e);
            }
        })
        .ok();
}

/// Something went wrong after the quick search box closed or without it
/// ever opening: say so on the main window. Not when the user moved to
/// another window themselves.
pub fn failed(app: &AppHandle, e: TypeError) {
    log::warn!("auto-type stopped: {e:?}");
    if e == TypeError::FocusLost {
        return;
    }
    windows::focus_main(app);
    // serialises as {"code": …}, like a command error
    let _ = app.emit("autotype-failed", app_error(e));
}
