//! Everything the frontend can call. The list is mirrored in build.rs and in
//! the per-window capabilities; see docs/architecture.md for the rules.

use std::path::{Path, PathBuf};
use std::sync::MutexGuard;

use mikoshi_core::model::{CustomIconView, EntryDetail, EntrySummary, GroupView, TotpView, fields};
use mikoshi_core::{Credentials, Secret, Vault};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::AppState;
use crate::error::{AppError, AppResult};
use crate::session::{Session, VaultStatus};
use crate::settings::{Settings, Store};
use crate::windows;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    /// The main window has the Mica backdrop, so its page must stay transparent
    pub effects: bool,
    /// Running on demo data (only the browser mock does now)
    pub demo: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
// Built again in M3, when copying reaches the clipboard.
#[allow(dead_code)]
pub struct CopyResult {
    /// seconds until the clipboard is cleared; `None` for non-secret fields
    pub clears_after: Option<u32>,
}

/// Texts for the native file dialog, in the UI's language.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogText {
    pub title: String,
    pub filter: String,
    pub all_files: String,
}

fn session<'a>(state: &'a State<'_, AppState>) -> AppResult<MutexGuard<'a, Session>> {
    state.vault.lock().map_err(|_| AppError::Internal)
}

fn settings<'a>(state: &'a State<'_, AppState>) -> AppResult<MutexGuard<'a, Store>> {
    state.settings.lock().map_err(|_| AppError::Internal)
}

/// Never holds both locks at once.
fn status_of(state: &State<'_, AppState>) -> AppResult<VaultStatus> {
    let current = session(state)?.current.clone();
    let key_file = match current {
        Some(p) => settings(state)?.current.key_file_for(&p.to_string_lossy()),
        None => None,
    };
    Ok(session(state)?.status(key_file))
}

fn announce(app: &AppHandle, state: &State<'_, AppState>) -> AppResult<VaultStatus> {
    let status = status_of(state)?;
    // keep every window in step
    let _ = app.emit("vault-changed", &status);
    Ok(status)
}

/// Runs `f` on the unlocked database. `Locked` when there is none,
/// `NotFound` when `f` finds nothing.
fn read<T>(state: &State<'_, AppState>, f: impl FnOnce(&Vault) -> Option<T>) -> AppResult<T> {
    let s = session(state)?;
    let vault = s.open.as_ref().ok_or(AppError::Locked)?;
    f(vault).ok_or(AppError::NotFound)
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        effects: state.effects,
        demo: false,
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(settings(&state)?.current.clone())
}

/// Saves what the user can change. The recent file list is not taken from
/// the frontend (see `Settings::merge_from_ui`).
#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<Settings> {
    let saved = {
        let mut store = self::settings(&state)?;
        let merged = store.current.merge_from_ui(settings);
        store.save(merged.clone())?;
        merged
    };
    let _ = app.emit("settings-changed", &saved);
    Ok(saved)
}

#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> AppResult<VaultStatus> {
    status_of(&state)
}

fn select(app: &AppHandle, state: &State<'_, AppState>, path: PathBuf) -> AppResult<VaultStatus> {
    if !path.is_file() {
        return Err(AppError::FileNotFound);
    }
    {
        let mut s = session(state)?;
        // pointing at another database locks the open one
        s.lock();
        s.current = Some(path);
    }
    announce(app, state)
}

/// Points the unlock screen at a database, e.g. one from the recent list.
#[tauri::command]
pub fn select_database(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<VaultStatus> {
    select(&app, &state, PathBuf::from(path))
}

/// Shows the native "open file" dialog. Must not run on the main thread,
/// hence async plus a blocking task.
async fn pick_file(
    app: AppHandle,
    window: tauri::Window,
    text: DialogText,
    extensions: &'static [&'static str],
    start: Option<PathBuf>,
) -> AppResult<Option<PathBuf>> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title(text.title)
            .add_filter(text.filter, extensions)
            .add_filter(text.all_files, &["*"]);
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        dialog.blocking_pick_file().and_then(|f| f.into_path().ok())
    })
    .await
    .map_err(|_| AppError::Internal)
}

fn folder_of(path: Option<&Path>) -> Option<PathBuf> {
    path.and_then(Path::parent).map(Path::to_path_buf)
}

/// "Open database…": picks a file and selects it. `None` when cancelled.
#[tauri::command]
pub async fn choose_database(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    text: DialogText,
) -> AppResult<Option<VaultStatus>> {
    let start = folder_of(session(&state)?.current.as_deref());
    match pick_file(app.clone(), window, text, &["kdbx"], start).await? {
        Some(path) => select(&app, &state, path).map(Some),
        None => Ok(None),
    }
}

/// Picks a key file. Only the path comes back; the file is read on unlock.
#[tauri::command]
pub async fn choose_key_file(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    text: DialogText,
) -> AppResult<Option<String>> {
    let start = folder_of(session(&state)?.current.as_deref());
    let picked = pick_file(app, window, text, &["keyx", "key"], start).await?;
    Ok(picked.map(|p| p.to_string_lossy().into_owned()))
}

/// Unlocks the selected database. Decryption (the KDF can take a second or
/// more on purpose) runs off the main thread so the windows stay responsive.
#[tauri::command]
pub async fn unlock(
    app: AppHandle,
    state: State<'_, AppState>,
    password: Option<Secret>,
    key_file: Option<String>,
) -> AppResult<VaultStatus> {
    let path = session(&state)?
        .current
        .clone()
        .ok_or(AppError::NoDatabase)?;
    let key_file = key_file.filter(|k| !k.trim().is_empty());

    let (file, key) = (path.clone(), key_file.clone());
    let vault = tauri::async_runtime::spawn_blocking(move || {
        // `password` is dropped (and wiped) at the end of this closure
        Vault::open(
            &file,
            Credentials {
                password: password.as_ref(),
                key_file: key.as_deref().map(Path::new),
            },
        )
    })
    .await
    .map_err(|_| AppError::Internal)?
    .map_err(|e| {
        log::info!("unlock failed: {e:?}");
        AppError::from(e)
    })?;

    {
        let mut s = session(&state)?;
        if s.current.as_deref() != Some(path.as_path()) {
            // another file was selected while this one was being decrypted
            return Err(AppError::Internal);
        }
        s.open = Some(vault);
    }
    log::info!("database unlocked");

    let path = path.to_string_lossy().into_owned();
    let saved = settings(&state)?.update(|s| s.note_opened(&path, key_file.as_deref()));
    match saved {
        Ok(s) => {
            let _ = app.emit("settings-changed", &s);
        }
        // the database is open anyway; only the recent list is stale
        Err(e) => log::warn!("recent files not saved: {e}"),
    }
    announce(&app, &state)
}

/// Locks: drops the decrypted database. Every window goes back to its
/// unlock form and forgets what it showed.
#[tauri::command]
pub fn lock(app: AppHandle, state: State<'_, AppState>) -> AppResult<VaultStatus> {
    session(&state)?.lock();
    log::info!("database locked");
    announce(&app, &state)
}

/// Removes a file from the recent list (the file itself is not touched).
#[tauri::command]
pub fn forget_recent(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<Settings> {
    let next = settings(&state)?.update(|s| s.forget(&path))?;
    let _ = app.emit("settings-changed", &next);
    let moved = {
        let mut s = session(&state)?;
        let selected = s
            .current
            .as_deref()
            .is_some_and(|c| c.to_string_lossy().eq_ignore_ascii_case(&path));
        if selected && s.open.is_none() {
            s.current = next.recent.first().map(|r| PathBuf::from(&r.path));
        }
        selected
    };
    if moved {
        announce(&app, &state)?;
    }
    Ok(next)
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> AppResult<Vec<GroupView>> {
    read(&state, |v| Some(v.groups()))
}

#[tauri::command]
pub fn list_entries(state: State<'_, AppState>, group_id: String) -> AppResult<Vec<EntrySummary>> {
    read(&state, |v| v.entries(&group_id))
}

#[tauri::command]
pub fn search_entries(state: State<'_, AppState>, query: String) -> AppResult<Vec<EntrySummary>> {
    read(&state, |v| Some(v.search(&query)))
}

#[tauri::command]
pub fn get_entry(state: State<'_, AppState>, id: String) -> AppResult<EntryDetail> {
    read(&state, |v| v.entry(&id))
}

#[tauri::command]
pub fn get_custom_icons(state: State<'_, AppState>) -> AppResult<Vec<CustomIconView>> {
    read(&state, |v| Some(v.custom_icons()))
}

/// The only command that sends a secret to the frontend: one value, on an
/// explicit user action (show password, start editing). The UI drops it again
/// when it is hidden or another entry is selected.
#[tauri::command]
pub fn reveal_field(
    state: State<'_, AppState>,
    entry_id: String,
    key: String,
) -> AppResult<String> {
    read(&state, |v| v.field(&entry_id, &key)).map(|s| s.expose().to_string())
}

/// Copies a field without sending it to the frontend. The clipboard (with
/// the history and cloud exclusions and the timed clear, features.md §3.5)
/// arrives in M3; until then this only checks that the field exists.
#[tauri::command]
pub fn copy_field(
    state: State<'_, AppState>,
    entry_id: String,
    key: String,
) -> AppResult<CopyResult> {
    if key == fields::TOTP {
        read(&state, |v| v.has_totp(&entry_id).filter(|&t| t))?;
    } else {
        read(&state, |v| v.is_secret_field(&entry_id, &key))?;
    }
    Err(AppError::NotYet)
}

/// TOTP codes are computed in M3 (the code parses otpauth URIs itself, see
/// M0 issues K3 and K4).
#[tauri::command]
pub fn get_totp(state: State<'_, AppState>, entry_id: String) -> AppResult<TotpView> {
    read(&state, |v| v.has_totp(&entry_id).filter(|&t| t))?;
    Err(AppError::NotYet)
}

#[tauri::command]
pub fn show_quick_search(app: AppHandle) -> AppResult<()> {
    windows::show_quick(&app).map_err(|e| {
        log::error!("cannot show quick search: {e}");
        AppError::Window
    })
}

#[tauri::command]
pub fn hide_quick_search(app: AppHandle) -> AppResult<()> {
    windows::hide_quick(&app).map_err(|e| {
        log::error!("cannot hide quick search: {e}");
        AppError::Window
    })
}
