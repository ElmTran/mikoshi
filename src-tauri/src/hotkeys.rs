//! Global hotkeys (features.md §3.6, §5): one shows the quick search box from
//! any program, one locks the database, one auto-types (§3.10).
//!
//! Settings store a hotkey as text: modifiers in the order Ctrl, Alt, Shift,
//! Win, then one key named like `KeyboardEvent.code` without its prefix
//! ("Ctrl+Alt+K", "Ctrl+Shift+F5", "Alt+Win+Space"). "" turns a hotkey off.
//! At least one of Ctrl, Alt or Win is required: a hotkey on a plain key or
//! Shift+key would swallow that key in every program.

use std::str::FromStr;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::session;
use crate::settings::Settings;
use crate::windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    QuickSearch,
    Lock,
    Autotype,
}

/// The hotkey texts from the settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wanted {
    pub quick_search: String,
    pub lock: String,
    pub autotype: String,
}

impl From<&Settings> for Wanted {
    fn from(s: &Settings) -> Self {
        Wanted {
            quick_search: s.quick_search_hotkey.clone(),
            lock: s.lock_hotkey.clone(),
            autotype: s.autotype_hotkey.clone(),
        }
    }
}

/// What became of a hotkey from the settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// turned off in the settings (or not set up yet)
    #[default]
    Off,
    Active,
    /// another program (or the other Mikoshi hotkey) already has it
    Taken,
    /// the text is not a usable hotkey
    Invalid,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub quick_search: State,
    pub lock: State,
    pub autotype: State,
}

impl Status {
    pub fn all_usable(&self) -> bool {
        [self.quick_search, self.lock, self.autotype]
            .iter()
            .all(|s| matches!(s, State::Off | State::Active))
    }
}

#[derive(Default)]
struct Inner {
    /// the texts from the settings, kept to re-register after a pause
    wanted: Wanted,
    registered: Vec<(Action, Shortcut)>,
    status: Status,
    paused: bool,
}

/// Managed state: which hotkeys are registered and for what.
#[derive(Default)]
pub struct Hotkeys(Mutex<Inner>);

/// Parses the settings format. `Ok(None)` for "" (off).
pub fn parse(text: &str) -> Result<Option<Shortcut>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let tokens: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, mods) = tokens.split_last().ok_or(())?;
    let mods: Vec<&str> = mods
        .iter()
        .map(|m| match m.to_ascii_lowercase().as_str() {
            "ctrl" => Ok("Ctrl"),
            "alt" => Ok("Alt"),
            "shift" => Ok("Shift"),
            "win" => Ok("Super"),
            _ => Err(()),
        })
        .collect::<Result<_, _>>()?;
    let shortcut =
        Shortcut::from_str(&[mods.as_slice(), &[key]].concat().join("+")).map_err(|_| ())?;
    if !shortcut
        .mods
        .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
    {
        return Err(());
    }
    Ok(Some(shortcut))
}

pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    if state != ShortcutState::Pressed {
        return;
    }
    let action = app.state::<Hotkeys>().0.lock().ok().and_then(|h| {
        h.registered
            .iter()
            .find(|(_, s)| s.id() == shortcut.id())
            .map(|(a, _)| *a)
    });
    match action {
        Some(Action::QuickSearch) => windows::toggle_quick(app),
        Some(Action::Lock) => session::lock_app(app, "hotkey"),
        Some(Action::Autotype) => crate::autotype::on_hotkey(app),
        None => {}
    }
}

/// Registers the hotkeys from the settings in place of the current ones and
/// tells every window the result.
pub fn apply(app: &AppHandle, wanted: Wanted) -> Status {
    let state = app.state::<Hotkeys>();
    let status = {
        let Ok(mut h) = state.0.lock() else {
            return Status::default();
        };
        h.wanted = wanted;
        // a recorded hotkey ends the pause, so the result is known at once
        h.paused = false;
        register(app, &mut h)
    };
    let _ = app.emit("hotkeys-changed", status);
    status
}

pub fn status(app: &AppHandle) -> Status {
    app.state::<Hotkeys>()
        .0
        .lock()
        .map(|h| h.status)
        .unwrap_or_default()
}

/// While the user records a new hotkey in the settings, the current ones must
/// not fire (and must not hide the keys being recorded).
pub fn pause(app: &AppHandle, paused: bool) -> Status {
    let state = app.state::<Hotkeys>();
    let Ok(mut h) = state.0.lock() else {
        return Status::default();
    };
    if h.paused == paused {
        return h.status;
    }
    h.paused = paused;
    if paused {
        unregister(app, &mut h);
        h.status
    } else {
        register(app, &mut h)
    }
}

fn unregister(app: &AppHandle, h: &mut Inner) {
    let gs = app.global_shortcut();
    for (_, s) in h.registered.drain(..) {
        let _ = gs.unregister(s);
    }
}

fn register(app: &AppHandle, h: &mut Inner) -> Status {
    unregister(app, h);
    // a test on another desktop must not react to the user's keys
    if crate::windows::dev_quiet() {
        log::info!("global hotkeys off: running on another desktop for a test");
        h.status = Status::default();
        return h.status;
    }
    let gs = app.global_shortcut();
    let wanted = h.wanted.clone();
    let mut one = |action: Action, text: &str| -> State {
        let shortcut = match parse(text) {
            Ok(None) => return State::Off,
            Ok(Some(s)) => s,
            Err(()) => {
                log::warn!("{action:?} hotkey is not valid: {text}");
                return State::Invalid;
            }
        };
        if h.registered.iter().any(|(_, s)| s.id() == shortcut.id()) {
            return State::Taken;
        }
        match gs.register(shortcut) {
            Ok(()) => {
                h.registered.push((action, shortcut));
                State::Active
            }
            Err(e) => {
                log::warn!("{action:?} hotkey {text} is not available: {e}");
                State::Taken
            }
        }
    };
    let status = Status {
        quick_search: one(Action::QuickSearch, &wanted.quick_search),
        lock: one(Action::Lock, &wanted.lock),
        autotype: one(Action::Autotype, &wanted.autotype),
    };
    h.status = status;
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_global_shortcut::Code;

    #[test]
    fn parses_the_settings_format() {
        let s = parse("Ctrl+Alt+K").unwrap().unwrap();
        assert_eq!(s.mods, Modifiers::CONTROL | Modifiers::ALT);
        assert_eq!(s.key, Code::KeyK);
        let s = parse("Alt+Win+Space").unwrap().unwrap();
        assert_eq!(s.mods, Modifiers::ALT | Modifiers::SUPER);
        assert_eq!(s.key, Code::Space);
        assert_eq!(parse("ctrl + shift + F5").unwrap().unwrap().key, Code::F5);
        assert_eq!(parse("Ctrl+Digit1").unwrap().unwrap().key, Code::Digit1);
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("  ").unwrap(), None);
    }

    #[test]
    fn rejects_unusable_hotkeys() {
        for bad in [
            "K",
            "Shift+K",
            "Ctrl+Alt",
            "Ctrl+Alt+",
            "Ctrl+Alt+K+L",
            "Hyper+K",
            "Super+K",
            "Ctrl+Nope",
        ] {
            assert_eq!(parse(bad), Err(()), "{bad}");
        }
    }
}
