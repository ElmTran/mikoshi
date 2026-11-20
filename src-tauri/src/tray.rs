//! The tray icon (features.md §3.6). Left click shows the main window; the
//! menu has show, lock or unlock, quick search and quit. Closing the main
//! window only hides it, so the tray's Quit is the way out.

use ::windows::Win32::Globalization::GetUserDefaultUILanguage;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, Wry};

use crate::AppState;
use crate::session;
use crate::settings::LanguagePref;
use crate::windows;

/// The menu items whose text changes.
struct Items {
    show: MenuItem<Wry>,
    lock: MenuItem<Wry>,
    quick: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

struct Texts {
    show: &'static str,
    lock: &'static str,
    unlock: &'static str,
    quick: &'static str,
    quit: &'static str,
}

const ZH: Texts = Texts {
    show: "显示主窗口",
    lock: "锁定",
    unlock: "解锁…",
    quick: "快速搜索",
    quit: "退出 Mikoshi",
};

const EN: Texts = Texts {
    show: "Show Mikoshi",
    lock: "Lock",
    unlock: "Unlock…",
    quick: "Quick search",
    quit: "Quit Mikoshi",
};

/// The same rule as the frontend: "follow the system" means Chinese when
/// Windows is in Chinese, English otherwise.
fn texts(app: &AppHandle) -> &'static Texts {
    let pref = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|s| s.current.language)
        .unwrap_or_default();
    let chinese = match pref {
        LanguagePref::ZhCn => true,
        LanguagePref::EnUs => false,
        // SAFETY: no preconditions; the primary language is the low 10 bits
        LanguagePref::System => (unsafe { GetUserDefaultUILanguage() } & 0x3ff) == 0x04,
    };
    if chinese { &ZH } else { &EN }
}

fn is_locked(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .vault
        .lock()
        .map(|s| s.open.is_none())
        .unwrap_or(true)
}

/// Unsaved edits are saved on the way out. If the user asked to be prompted
/// when locking with unsaved edits, the main window gets `quit-requested`
/// and offers save / discard / cancel instead; it calls `quit_app` itself.
fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    if crate::session::needs_answer(&state) {
        windows::focus_main(app);
        let _ = app.emit("quit-requested", ());
        return;
    }
    crate::session::leave(app, false);
    app.exit(0);
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let t = texts(app);
    let items = Items {
        show: MenuItem::with_id(app, "show", t.show, true, None::<&str>)?,
        lock: MenuItem::with_id(app, "lock", t.unlock, true, None::<&str>)?,
        quick: MenuItem::with_id(app, "quick", t.quick, true, None::<&str>)?,
        quit: MenuItem::with_id(app, "quit", t.quit, true, None::<&str>)?,
    };
    let menu = Menu::with_items(
        app,
        &[
            &items.show,
            &items.lock,
            &items.quick,
            &PredefinedMenuItem::separator(app)?,
            &items.quit,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Mikoshi")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => windows::focus_main(app),
            "lock" => {
                if is_locked(app) {
                    // unlocking happens on the main window's unlock screen
                    windows::focus_main(app);
                } else {
                    session::lock_app(app, "tray");
                }
            }
            "quick" => {
                if let Err(e) = windows::show_quick(app) {
                    log::error!("cannot show quick search: {e}");
                }
            }
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                windows::focus_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(items);

    // keep the texts in step with the lock state and the language
    for event in ["vault-changed", "settings-changed"] {
        let app2 = app.clone();
        app.listen_any(event, move |_| refresh(&app2));
    }
    refresh(app);
    Ok(())
}

fn refresh(app: &AppHandle) {
    let t = texts(app);
    let items = app.state::<Items>();
    let _ = items.show.set_text(t.show);
    let _ = items
        .lock
        .set_text(if is_locked(app) { t.unlock } else { t.lock });
    let _ = items.quick.set_text(t.quick);
    let _ = items.quit.set_text(t.quit);
}
