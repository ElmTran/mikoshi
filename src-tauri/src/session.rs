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

/// Locks: drops the decrypted database, removes a copied secret from the
/// clipboard if it is still there, and tells every window. Used by the lock
/// command and by auto-lock (idle, Win+L, sleep, minimize).
pub fn lock_app(app: &AppHandle, reason: &str) {
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
