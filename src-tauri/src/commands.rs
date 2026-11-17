//! Everything the frontend can call. The list is mirrored in build.rs and in
//! the per-window capabilities; see docs/architecture.md for the rules.

use std::sync::MutexGuard;

use mikoshi_core::Secret;
use mikoshi_core::model::{EntryDetail, EntrySummary, GroupView, TotpView, fields};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::AppState;
use crate::demo::{DemoVault, VaultStatus};
use crate::error::{AppError, AppResult};
use crate::settings::Settings;
use crate::windows;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    /// The main window has the Mica backdrop, so its page must stay transparent
    pub effects: bool,
    /// The sample runs on demo data and does not touch the clipboard yet
    pub demo: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    /// seconds until the clipboard is cleared; `None` for non-secret fields
    pub clears_after: Option<u32>,
}

fn vault<'a>(state: &'a State<'_, AppState>) -> AppResult<MutexGuard<'a, DemoVault>> {
    state.vault.lock().map_err(|_| AppError::Internal)
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        effects: state.effects,
        demo: true,
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(state
        .settings
        .lock()
        .map_err(|_| AppError::Internal)?
        .current
        .clone())
}

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<Settings> {
    state
        .settings
        .lock()
        .map_err(|_| AppError::Internal)?
        .save(settings.clone())?;
    // keep every window in step (theme, language)
    let _ = app.emit("settings-changed", &settings);
    Ok(settings)
}

#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> AppResult<VaultStatus> {
    Ok(vault(&state)?.status())
}

#[tauri::command]
pub fn unlock(
    app: AppHandle,
    state: State<'_, AppState>,
    password: Secret,
) -> AppResult<VaultStatus> {
    let status = {
        let mut v = vault(&state)?;
        v.unlock(&password)?;
        v.status()
    };
    // `password` is wiped when it goes out of scope
    let _ = app.emit("vault-changed", &status);
    Ok(status)
}

#[tauri::command]
pub fn lock(app: AppHandle, state: State<'_, AppState>) -> AppResult<VaultStatus> {
    let status = {
        let mut v = vault(&state)?;
        v.lock();
        v.status()
    };
    let _ = app.emit("vault-changed", &status);
    Ok(status)
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> AppResult<Vec<GroupView>> {
    vault(&state)?.groups()
}

#[tauri::command]
pub fn list_entries(state: State<'_, AppState>, group_id: String) -> AppResult<Vec<EntrySummary>> {
    vault(&state)?.entries(&group_id)
}

#[tauri::command]
pub fn search_entries(state: State<'_, AppState>, query: String) -> AppResult<Vec<EntrySummary>> {
    vault(&state)?.search(&query)
}

#[tauri::command]
pub fn get_entry(state: State<'_, AppState>, id: String) -> AppResult<EntryDetail> {
    vault(&state)?.entry(&id)
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
    Ok(vault(&state)?.field(&entry_id, &key)?.expose().to_string())
}

/// Copies a field without sending it to the frontend. In the M1 sample this
/// only checks that the field exists; the clipboard (with the history and
/// cloud exclusions and the timed clear, features.md §3.5) arrives in M3.
#[tauri::command]
pub fn copy_field(
    state: State<'_, AppState>,
    entry_id: String,
    key: String,
) -> AppResult<CopyResult> {
    let v = vault(&state)?;
    // checks that the field exists; the value itself goes nowhere yet
    v.field(&entry_id, &key)?;
    let secret = match key.as_str() {
        fields::PASSWORD | fields::TOTP => true,
        fields::TITLE | fields::USERNAME | fields::URL | fields::NOTES => false,
        custom => v
            .entry(&entry_id)?
            .fields
            .iter()
            .any(|f| f.key == custom && f.protected),
    };
    Ok(CopyResult {
        clears_after: secret.then_some(10),
    })
}

#[tauri::command]
pub fn get_totp(state: State<'_, AppState>, entry_id: String) -> AppResult<TotpView> {
    vault(&state)?.totp(&entry_id)
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
