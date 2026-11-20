//! Which database is selected, and the unlocked one if any.

use std::path::{Path, PathBuf};

use mikoshi_core::Vault;
use mikoshi_core::vault::file_stem;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

/// What every window needs to know about the database. Paths are not
/// secret; nothing from inside the database is here except its name.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub locked: bool,
    /// the selected database file; `None` before the first one is chosen
    pub path: Option<String>,
    pub file: Option<String>,
    /// the database's own name once unlocked, the file name before
    pub name: String,
    /// the key file remembered for this database, to fill in the unlock form
    pub key_file: Option<String>,
    /// edits not saved yet (M7); the title bar shows a dot for this
    pub dirty: bool,
}

#[derive(Default)]
pub struct Session {
    /// the database the unlock screen points at
    pub current: Option<PathBuf>,
    /// the unlocked database. Dropping it is locking: keepass-rs wipes the
    /// protected values it holds.
    pub open: Option<Vault>,
}

impl Session {
    pub fn new(current: Option<PathBuf>) -> Session {
        Session {
            current,
            open: None,
        }
    }

    pub fn lock(&mut self) {
        self.open = None;
    }

    pub fn status(&self, key_file: Option<String>) -> VaultStatus {
        let path = self.current.as_deref();
        let file = path
            .and_then(Path::file_name)
            .map(|f| f.to_string_lossy().into_owned());
        VaultStatus {
            locked: self.open.is_none(),
            path: path.map(|p| p.to_string_lossy().into_owned()),
            file,
            name: match (&self.open, path) {
                (Some(v), _) => v.name(),
                (None, Some(p)) => file_stem(p),
                (None, None) => String::new(),
            },
            key_file,
            dirty: self.open.as_ref().is_some_and(Vault::is_dirty),
        }
    }
}

/// The status every window shows. Never holds both locks at once.
pub fn status_of(state: &AppState) -> Option<VaultStatus> {
    let current = state.vault.lock().ok()?.current.clone();
    let key_file = match current {
        Some(p) => state
            .settings
            .lock()
            .ok()?
            .current
            .key_file_for(&p.to_string_lossy()),
        None => None,
    };
    Some(state.vault.lock().ok()?.status(key_file))
}

/// Locks: saves unsaved edits (unless `save` is false), drops the decrypted
/// database, removes a copied secret from the clipboard if it is still there,
/// and tells every window. Used by the lock command, the tray and auto-lock
/// (idle, Win+L, sleep, minimize).
pub fn lock_app(app: &AppHandle, reason: &str) {
    lock_app_with(app, reason, true);
}

/// [`lock_app`], where `save == false` drops unsaved edits instead.
pub fn lock_app_with(app: &AppHandle, reason: &str, save: bool) {
    if save {
        note_conflict_copy(app, save_before_leaving(app));
    }
    let state = app.state::<AppState>();
    let was_open = match state.vault.lock() {
        Ok(mut s) => {
            let open = s.open.is_some();
            s.lock();
            open
        }
        Err(_) => false,
    };
    crate::clipboard::clear_secret(app);
    if was_open {
        log::info!("database locked ({reason})");
        if let Some(status) = status_of(&state) {
            let _ = app.emit("vault-changed", &status);
        }
    }
}

/// Tells the windows where edits went when they could not be saved over the
/// file itself. The path is not a secret (it is on the recent list anyway).
fn note_conflict_copy(app: &AppHandle, path: Option<std::path::PathBuf>) {
    if let Some(path) = path {
        let path = path.to_string_lossy().into_owned();
        let _ = app.emit("save-conflict-copy", &path);
    }
}

/// Whether the database has unsaved edits the user has to decide about:
/// the setting says "ask" and there are edits not saved yet.
pub fn needs_answer(state: &AppState) -> bool {
    let ask = state
        .settings
        .lock()
        .is_ok_and(|s| s.current.lock_unsaved == crate::settings::LockUnsaved::Ask);
    ask && is_dirty(state)
}

/// Before leaving a database: saves unsaved edits unless they were to be
/// discarded, and tells the windows about a conflict copy.
pub fn leave(app: &AppHandle, discard: bool) {
    if !discard {
        note_conflict_copy(app, save_before_leaving(app));
    }
}

/// Whether the open database has edits not saved yet.
pub fn is_dirty(state: &AppState) -> bool {
    state
        .vault
        .lock()
        .is_ok_and(|s| s.open.as_ref().is_some_and(Vault::is_dirty))
}

/// Saves the open database's unsaved edits before it is dropped, for an
/// automatic lock, quitting or switching databases, where nobody can be
/// asked. A conflict with someone else's change to the file is not
/// overwritten: the edits go to a separate file (see
/// `Vault::save_conflict_copy`) and its path is returned so the user can be
/// told. Failures are logged; the caller goes ahead either way.
pub fn save_before_leaving(app: &AppHandle) -> Option<std::path::PathBuf> {
    let state = app.state::<AppState>();
    let backup = state
        .settings
        .lock()
        .is_ok_and(|s| s.current.backup_on_save);
    let mut s = state.vault.lock().ok()?;
    let vault = s.open.as_mut()?;
    if !vault.is_dirty() {
        return None;
    }
    match vault.save(mikoshi_core::vault::SaveOptions {
        backup,
        ..Default::default()
    }) {
        Ok(()) => {
            log::info!("unsaved edits saved before leaving");
            None
        }
        Err(mikoshi_core::vault::SaveError::Conflict) => match vault.save_conflict_copy() {
            Ok(path) => {
                log::info!("the file changed elsewhere; edits saved to a separate file");
                Some(path)
            }
            Err(e) => {
                log::warn!("edits could not be saved anywhere: {e:?}");
                None
            }
        },
        Err(e) => {
            log::warn!("edits could not be saved: {e:?}");
            None
        }
    }
}
