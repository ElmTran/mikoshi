mod autolock;
mod autotype;
mod browser;
mod clipboard;
mod commands;
mod error;
mod hotkeys;
mod session;
mod settings;
mod tray;
mod typing;
mod windows;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_log::{Target, TargetKind};

pub struct AppState {
    pub settings: Mutex<settings::Store>,
    pub vault: Mutex<session::Session>,
    /// what we last put on the clipboard, to clear it again safely
    pub clipboard: clipboard::Tracker,
    /// the main window has the Mica backdrop, so its page must stay transparent
    pub effects: bool,
    /// started with Windows (`--hidden`): stay in the tray, the main window
    /// is not shown until asked for
    pub start_hidden: bool,
}

/// The argument start-with-Windows passes, see `start_hidden`.
const HIDDEN_ARG: &str = "--hidden";

/// WebView2 reads a few environment variables that change how it starts:
/// extra browser arguments (which can open a remote debugging port), another
/// browser binary, another profile folder. A release build ignores them all,
/// so setting an environment variable is not enough to attach a debugger to
/// Mikoshi or to swap the runtime under it. Debug builds keep them: that is
/// how tools/cdp.mjs drives the app during development.
#[cfg(not(debug_assertions))]
fn ignore_webview2_overrides() {
    for name in [
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
        "WEBVIEW2_USER_DATA_FOLDER",
        "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
    ] {
        // SAFETY: called first thing in `run`, before any other thread exists.
        unsafe { std::env::remove_var(name) };
    }
}

pub fn run() {
    #[cfg(not(debug_assertions))]
    ignore_webview2_overrides();

    tauri::Builder::default()
        // Must be registered first: a second launch hands over to the running
        // instance (which comes to the front) and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::focus_main(app)
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    hotkeys::on_shortcut(app, shortcut, event.state)
                })
                .build(),
        )
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg(HIDDEN_ARG)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                ])
                .build(),
        )
        .setup(|app| {
            // Order matters. A page starts calling commands while its window
            // is still being created, so everything the commands need must be
            // in place before the first window exists. (The windows have
            // `"create": false` in tauri.conf.json for this reason. Getting it
            // wrong only shows in production builds, where pages load at once:
            // the first calls fail and the window stays blank.)
            let config_dir = app.path().app_config_dir()?;
            let effects = windows::supports_mica();
            let store = settings::Store::load(config_dir);
            // start on the database used last (features.md §3.1)
            let last = store.current.recent.first().map(|r| PathBuf::from(&r.path));
            app.manage(hotkeys::Hotkeys::default());
            app.manage(windows::PreviousWindow::default());
            let keys = hotkeys::apply(app.handle(), hotkeys::Wanted::from(&store.current));
            // a hotkey that did not register would leave a quiet start with
            // no way in except the tray: show the window and its warning
            let start_hidden = std::env::args().any(|a| a == HIDDEN_ARG) && keys.all_usable();
            app.manage(AppState {
                settings: Mutex::new(store),
                vault: Mutex::new(session::Session::new(last)),
                clipboard: clipboard::Tracker::default(),
                effects,
                start_hidden,
            });
            windows::create_all(app.handle())?;
            tray::create(app.handle())?;
            autolock::start(app.handle());

            log::info!(
                "Mikoshi {} started (window effects: {effects})",
                env!("CARGO_PKG_VERSION")
            );
            if !start_hidden {
                windows::show_main_if_frontend_is_stuck(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                // the program lives on in the tray; Quit is in the tray menu
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    if let Some(w) = window.app_handle().get_webview_window("main") {
                        let _ = windows::hide(&w);
                    }
                }
                // optional: lock when minimized (features.md §3.2)
                WindowEvent::Resized(_) if window.is_minimized().unwrap_or(false) => {
                    let app = window.app_handle();
                    let on = app
                        .state::<AppState>()
                        .settings
                        .lock()
                        .is_ok_and(|s| s.current.lock_on_minimize);
                    if on {
                        session::lock_app(app, "minimized");
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_settings,
            commands::set_settings,
            commands::vault_status,
            commands::select_database,
            commands::choose_database,
            commands::choose_key_file,
            commands::unlock,
            commands::lock,
            commands::forget_recent,
            commands::list_groups,
            commands::list_entries,
            commands::search_entries,
            commands::get_entry,
            commands::get_custom_icons,
            commands::reveal_field,
            commands::copy_field,
            commands::get_totp,
            commands::open_url,
            commands::show_quick_search,
            commands::hide_quick_search,
            commands::autotype_candidates,
            commands::autotype_entry,
            commands::autotype_from_main,
            commands::hotkey_status,
            commands::pause_hotkeys,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting Mikoshi")
        .run(|app, event| {
            // a copied password must not outlive the program (features.md §3.5)
            if let RunEvent::Exit = event {
                clipboard::clear_secret(app);
            }
        });
}
