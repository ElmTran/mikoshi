//! Everything the frontend can call. The list is mirrored in build.rs and in
//! the per-window capabilities; see docs/architecture.md for the rules.

use std::path::{Path, PathBuf};
use std::sync::MutexGuard;

use mikoshi_core::generator::{self, GeneratorOptions, Strength};
use mikoshi_core::model::{CustomIconView, EntryDetail, EntrySummary, GroupView, TotpView};
use mikoshi_core::totp::TotpError;
use mikoshi_core::vault::Deleted;
use mikoshi_core::vault::{
    EditError, EditableEntry, EntryEdit, HistoryItem, SaveError, SaveOptions,
};
use mikoshi_core::{Credentials, Secret, Vault};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::AppState;
use crate::autotype;
use crate::clipboard;
use crate::error::{AppError, AppResult};
use crate::hotkeys;
use crate::session::{self, Session, VaultStatus};
use crate::settings::{Settings, Store};
use crate::typing;
use crate::windows;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    /// Running on demo data (only the browser mock does now)
    pub demo: bool,
    /// Started with Windows: the main window stays hidden until asked for
    pub start_hidden: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
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
    session::status_of(state).ok_or(AppError::Internal)
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

/// Runs an edit on the unlocked database and tells the windows the views
/// changed. `Err` from `f` changes nothing.
fn edit_vault<T>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    f: impl FnOnce(&mut Vault) -> Result<T, EditError>,
) -> AppResult<T> {
    let out = {
        let mut s = session(state)?;
        let vault = s.open.as_mut().ok_or(AppError::Locked)?;
        f(vault)?
    };
    let status = status_of(state)?;
    let _ = app.emit("vault-edited", &status);
    Ok(out)
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        demo: false,
        start_hidden: state.start_hidden,
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(settings(&state)?.current.clone())
}

/// Saves what the user can change. The recent file list is not taken from
/// the frontend (see `Settings::merge_from_ui`).
///
/// A changed hotkey is registered first. If it is not usable nothing is
/// saved, the old hotkeys stay, and the error says why.
#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<Settings> {
    let (merged, old) = {
        let store = self::settings(&state)?;
        let c = &store.current;
        (c.merge_from_ui(settings), hotkeys::Wanted::from(c))
    };
    let keys = hotkeys::Wanted::from(&merged);
    let keys_changed = keys != old;
    if keys_changed {
        let status = hotkeys::apply(&app, keys.clone());
        let problem = |changed: bool, s: hotkeys::State| match s {
            hotkeys::State::Taken if changed => Some(AppError::HotkeyTaken),
            hotkeys::State::Invalid if changed => Some(AppError::HotkeyInvalid),
            _ => None,
        };
        let err = problem(keys.quick_search != old.quick_search, status.quick_search)
            .or_else(|| problem(keys.lock != old.lock, status.lock))
            .or_else(|| problem(keys.autotype != old.autotype, status.autotype));
        if let Some(err) = err {
            hotkeys::apply(&app, old);
            return Err(err);
        }
    }
    let saved = self::settings(&state)?
        .save(merged.clone())
        .map(|()| merged);
    let saved = match saved {
        Ok(s) => s,
        Err(e) => {
            if keys_changed {
                hotkeys::apply(&app, old);
            }
            return Err(e);
        }
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
/// With unsaved edits, `discard` says they may be dropped; the setting
/// "save before locking" saves them instead (a change made elsewhere by
/// another program gets a separate file, see `session::save_before_leaving`).
#[tauri::command]
pub fn select_database(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    discard: Option<bool>,
) -> AppResult<VaultStatus> {
    let discard = discard.unwrap_or(false);
    if session::needs_answer(&state) && !discard {
        return Err(AppError::UnsavedChanges);
    }
    session::leave(&app, discard);
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
/// unlock form and forgets what it showed. With unsaved edits, `discard`
/// says they may be dropped; otherwise the setting decides (save, or ask
/// with `UnsavedChanges` so the UI can offer the three choices).
#[tauri::command]
pub fn lock(
    app: AppHandle,
    state: State<'_, AppState>,
    discard: Option<bool>,
) -> AppResult<VaultStatus> {
    let discard = discard.unwrap_or(false);
    if session::needs_answer(&state) && !discard {
        return Err(AppError::UnsavedChanges);
    }
    session::lock_app_with(&app, "by the user", !discard);
    status_of(&state)
}

/// Quits from the main window, or after the tray asked the user. Unsaved
/// edits are saved unless `discard`; `UnsavedChanges` when the setting says
/// to ask first.
#[tauri::command]
pub fn quit_app(
    app: AppHandle,
    state: State<'_, AppState>,
    discard: Option<bool>,
) -> AppResult<()> {
    let discard = discard.unwrap_or(false);
    if session::needs_answer(&state) && !discard {
        return Err(AppError::UnsavedChanges);
    }
    session::leave(&app, discard);
    app.exit(0);
    Ok(())
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

fn totp_error(e: TotpError) -> AppError {
    match e {
        TotpError::Missing => AppError::NotFound,
        TotpError::Invalid => AppError::TotpInvalid,
    }
}

// ---------------------------------------------------------------- editing (M7)

/// The entry with every value, for the editor. The second half of the only
/// exception to "no secrets in the frontend": called when the editor opens,
/// its result is dropped when the editor closes or the database locks.
#[tauri::command]
pub fn get_entry_for_edit(
    state: State<'_, AppState>,
    entry_id: String,
) -> AppResult<EditableEntry> {
    read(&state, |v| v.editable(&entry_id))
}

/// Creates an entry in a group and returns its id. Nothing is written to the
/// file until `save_database`.
#[tauri::command]
pub fn create_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: String,
    edit: EntryEdit,
) -> AppResult<String> {
    edit_vault(&app, &state, |v| v.create_entry(&group_id, &edit))
}

/// Applies an edit; `false` when it changed nothing. Nothing is written to
/// the file until `save_database`.
#[tauri::command]
pub fn save_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
    edit: EntryEdit,
) -> AppResult<bool> {
    edit_vault(&app, &state, |v| v.update_entry(&entry_id, &edit))
}

/// Older versions of an entry, oldest first.
#[tauri::command]
pub fn list_history(state: State<'_, AppState>, entry_id: String) -> AppResult<Vec<HistoryItem>> {
    read(&state, |v| v.history(&entry_id))
}

/// One older version, shown like an entry (no secrets).
#[tauri::command]
pub fn get_history_entry(
    state: State<'_, AppState>,
    entry_id: String,
    index: usize,
) -> AppResult<EntryDetail> {
    read(&state, |v| v.history_entry(&entry_id, index))
}

/// One field of an older version, on "show password" in the history view.
#[tauri::command]
pub fn reveal_history_field(
    state: State<'_, AppState>,
    entry_id: String,
    index: usize,
    key: String,
) -> AppResult<String> {
    read(&state, |v| v.history_field(&entry_id, index, &key)).map(|s| s.expose().to_string())
}

/// Makes an older version the current one; the current one goes into the
/// history.
#[tauri::command]
pub fn restore_history(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
    index: usize,
) -> AppResult<VaultStatus> {
    edit_vault(&app, &state, |v| v.restore_history(&entry_id, index))?;
    status_of(&state)
}

/// Writes the database to its file. `conflict` when another program changed
/// the file meanwhile; the UI asks, then retries with `force`. `formatUpgrade`
/// when the file is KDBX 3.1 or 4.0 and the UI has not confirmed the change
/// to 4.1 yet.
///
/// The KDF runs on a background thread, on a copy of the database: the vault
/// is locked only to start and to finish, so the windows stay responsive and
/// other commands keep working. Edits made meanwhile stay unsaved.
#[tauri::command]
pub async fn save_database(
    app: AppHandle,
    state: State<'_, AppState>,
    options: Option<SaveOptions>,
) -> AppResult<VaultStatus> {
    let mut opts = options.unwrap_or_default();
    opts.backup = settings(&state)?.current.backup_on_save;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || save_now(&app2, opts))
        .await
        .map_err(|_| AppError::Internal)??;
    announce(&app, &state)
}

fn log_save_error(e: &SaveError) {
    match e {
        SaveError::Conflict => log::info!("save refused: the file changed elsewhere"),
        SaveError::Encrypt => log::warn!("save failed: the file does not read back"),
        SaveError::Write => log::warn!("save failed: cannot write the file"),
        SaveError::FormatUpgrade => {}
    }
}

/// One whole save, on the calling thread (see `save_database`).
pub fn save_now(app: &AppHandle, opts: SaveOptions) -> AppResult<()> {
    let state = tauri::Manager::state::<AppState>(app);
    let _one_at_a_time = state.save_lock.lock().map_err(|_| AppError::Internal)?;
    let job = {
        let s = state.vault.lock().map_err(|_| AppError::Internal)?;
        let vault = s.open.as_ref().ok_or(AppError::Locked)?;
        vault.prepare_save(opts).inspect_err(log_save_error)?
    };
    let path = job.path().to_path_buf();
    let saved = job.run().inspect_err(log_save_error)?;
    let mut s = state.vault.lock().map_err(|_| AppError::Internal)?;
    // locked, or another database opened while this one was being written:
    // the file is saved, there is just no vault left to tell
    if let Some(vault) = s.open.as_mut().filter(|v| v.path() == path) {
        vault.finish_save(saved);
    }
    log::info!("database saved");
    Ok(())
}

/// Reads the file again, dropping unsaved edits.
#[tauri::command]
pub fn reload_database(app: AppHandle, state: State<'_, AppState>) -> AppResult<VaultStatus> {
    {
        let mut s = session(&state)?;
        let vault = s.open.as_mut().ok_or(AppError::Locked)?;
        vault.reload()?;
    }
    announce(&app, &state)
}

/// Copies a field (or `@totp`, the current code) without sending it to the
/// frontend. Secrets stay out of clipboard history and are cleared after the
/// configured time (features.md §3.5); see clipboard.rs.
#[tauri::command]
pub fn copy_field(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
    key: String,
) -> AppResult<CopyResult> {
    let (value, secret) = read(&state, |v| v.copy_value(&entry_id, &key))?.map_err(totp_error)?;
    if value.is_empty() {
        // nothing to copy; leave whatever is on the clipboard alone
        return Err(AppError::NotFound);
    }
    let seconds = settings(&state)?.current.clipboard_clear_seconds;
    let clears_after = (secret && seconds > 0).then_some(seconds);
    clipboard::copy(&app, &value, secret, clears_after)?;
    Ok(CopyResult { clears_after })
}

/// The current TOTP code, computed here; the secret never leaves Rust.
#[tauri::command]
pub fn get_totp(state: State<'_, AppState>, entry_id: String) -> AppResult<TotpView> {
    read(&state, |v| v.totp(&entry_id))?.map_err(totp_error)
}

/// The URL to open for what an entry stores: only http and https. A bare
/// host gets `https://`. KeePass also allows `cmd://` and the like; those
/// run programs, so they are refused.
fn safe_url(raw: &str) -> Option<url::Url> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return None;
    }
    let candidate = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let url = url::Url::parse(&candidate).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host().is_some()).then_some(url)
}

/// Opens the entry's URL in the default browser.
#[tauri::command]
pub fn open_url(app: AppHandle, state: State<'_, AppState>, entry_id: String) -> AppResult<()> {
    let raw = read(&state, |v| v.url(&entry_id))?;
    if raw.is_empty() {
        return Err(AppError::NotFound);
    }
    let url = safe_url(&raw).ok_or(AppError::UnsafeUrl)?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|_| {
            // no details: they would contain the URL
            log::warn!("the browser could not be started");
            AppError::Internal
        })
}

#[tauri::command]
pub fn show_quick_search(app: AppHandle) -> AppResult<()> {
    windows::show_quick(&app).map_err(|e| {
        log::error!("cannot show quick search: {e}");
        AppError::Window
    })
}

/// `restore`: give the focus back to the window that was in front before
/// (after copying, Esc); not when the box lost the focus to a click.
#[tauri::command]
pub fn hide_quick_search(app: AppHandle, restore: bool) -> AppResult<()> {
    windows::hide_quick(&app, restore).map_err(|e| {
        log::error!("cannot hide quick search: {e}");
        AppError::Window
    })
}

/// Entries that fit the window the quick search box was opened over, for
/// picking one to auto-type (the box's `pick` mode).
#[tauri::command]
pub fn autotype_candidates(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<EntrySummary>> {
    let Some(target) = windows::previous(&app) else {
        return Ok(Vec::new());
    };
    let title = typing::window_title(target);
    let url = crate::browser::page_url(target);
    read(&state, |v| {
        Some(
            v.autotype_matches(&title, url.as_deref())
                .into_iter()
                .map(|(e, _)| e)
                .collect(),
        )
    })
}

/// From the quick search box: hides it and types into the window it was
/// opened over. The entry's sequence (`Enter`), or only the password
/// (`Ctrl+Enter`). Everything that can fail before typing fails here, while
/// the box can still show it.
#[tauri::command]
pub fn autotype_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
    password_only: bool,
) -> AppResult<()> {
    let target = windows::previous(&app).ok_or(AppError::AutotypeNoWindow)?;
    let plan = {
        let s = session(&state)?;
        let vault = s.open.as_ref().ok_or(AppError::Locked)?;
        let sequence = if password_only {
            "{PASSWORD}".to_string()
        } else {
            vault
                .autotype_sequence(&entry_id, &typing::window_title(target))
                .ok_or(AppError::NotFound)?
                .map_err(|_| AppError::AutotypeDisabled)?
        };
        typing::plan(vault, &entry_id, &sequence, target).map_err(autotype::app_error)?
    };
    windows::hide_quick(&app, true).map_err(|e| {
        log::error!("cannot hide quick search: {e}");
        AppError::Window
    })?;
    autotype::start(&app, plan);
    Ok(())
}

/// From the main window (`Ctrl+Shift+V`, the auto-type button): Mikoshi
/// minimizes, and the window that comes to the front gets the entry's
/// sequence. Problems after that show up as an `autotype-failed` event.
#[tauri::command]
pub fn autotype_from_main(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
) -> AppResult<()> {
    // fail early, while the main window is still there to say so
    read(&state, |v| v.autotype_sequence(&entry_id, ""))?
        .map_err(|_| AppError::AutotypeDisabled)?;
    if let Some(w) = tauri::Manager::get_webview_window(&app, "main") {
        let _ = w.minimize();
    }
    std::thread::spawn(move || {
        // Windows brings the next window forward once ours is minimized
        std::thread::sleep(std::time::Duration::from_millis(300));
        let Some(target) = typing::foreground_window() else {
            return autotype::failed(&app, typing::TypeError::NoWindow);
        };
        let title = typing::window_title(target);
        let state = tauri::Manager::state::<AppState>(&app);
        let plan = state.vault.lock().ok().and_then(|s| {
            s.open
                .as_ref()
                .map(|v| match v.autotype_sequence(&entry_id, &title) {
                    Some(Ok(seq)) => typing::plan(v, &entry_id, &seq, target),
                    _ => Err(typing::TypeError::BadSequence),
                })
        });
        match plan {
            Some(Ok(plan)) => autotype::start(&app, plan),
            Some(Err(e)) => autotype::failed(&app, e),
            // locked in the meantime
            None => autotype::failed(&app, typing::TypeError::Rejected),
        }
    });
    Ok(())
}

#[tauri::command]
pub fn hotkey_status(app: AppHandle) -> hotkeys::Status {
    hotkeys::status(&app)
}

/// Turns the global hotkeys off while the settings record a new one, and back
/// on. Saving a hotkey also turns them back on.
#[tauri::command]
pub fn pause_hotkeys(app: AppHandle, paused: bool) -> hotkeys::Status {
    hotkeys::pause(&app, paused)
}

/// Start with Windows. Windows keeps this setting (the Run key, and Task
/// Manager can turn it off), so it is read from there, not from settings.json.
#[tauri::command]
pub fn get_autostart(app: AppHandle) -> AppResult<bool> {
    app.autolaunch().is_enabled().map_err(|e| {
        log::error!("cannot read start with Windows: {e}");
        AppError::Autostart
    })
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> AppResult<bool> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| {
        log::error!("cannot change start with Windows: {e}");
        AppError::Autostart
    })?;
    get_autostart(app)
}

// ---------------------------------------------------------------- M8: organizing

/// Shows the native "save as" dialog. Must not run on the main thread.
async fn pick_save_path(
    app: AppHandle,
    window: tauri::Window,
    text: DialogText,
    extensions: &'static [&'static str],
    file_name: Option<String>,
    start: Option<PathBuf>,
) -> AppResult<Option<PathBuf>> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title(text.title);
        if !extensions.is_empty() {
            dialog = dialog.add_filter(text.filter, extensions);
        }
        dialog = dialog.add_filter(text.all_files, &["*"]);
        if let Some(name) = file_name {
            dialog = dialog.set_file_name(name);
        }
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        dialog.blocking_save_file().and_then(|f| f.into_path().ok())
    })
    .await
    .map_err(|_| AppError::Internal)
}

/// Whether deleting these removes them for good (in the recycle bin, or the
/// bin is turned off): the UI asks first.
#[tauri::command]
pub fn deletes_permanently(
    state: State<'_, AppState>,
    entry_ids: Vec<String>,
    group_ids: Vec<String>,
) -> AppResult<bool> {
    read(&state, |v| {
        Some(v.deletes_permanently(&entry_ids, &group_ids))
    })
}

/// Deletes entries: into the recycle bin, or for good from inside it.
/// `bin_name` names the bin when it has to be made (in the UI's language).
#[tauri::command]
pub fn delete_entries(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_ids: Vec<String>,
    bin_name: String,
) -> AppResult<Deleted> {
    edit_vault(&app, &state, |v| v.delete_entries(&entry_ids, &bin_name))
}

#[tauri::command]
pub fn delete_group(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: String,
    bin_name: String,
) -> AppResult<Deleted> {
    edit_vault(&app, &state, |v| v.delete_group(&group_id, &bin_name))
}

/// Takes an entry or group out of the recycle bin.
#[tauri::command]
pub fn restore_item(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    edit_vault(&app, &state, |v| v.restore(&id))
}

#[tauri::command]
pub fn empty_recycle_bin(app: AppHandle, state: State<'_, AppState>) -> AppResult<usize> {
    edit_vault(&app, &state, Vault::empty_recycle_bin)
}

#[tauri::command]
pub fn create_group(
    app: AppHandle,
    state: State<'_, AppState>,
    parent_id: String,
    name: String,
) -> AppResult<String> {
    edit_vault(&app, &state, |v| v.create_group(&parent_id, &name))
}

#[tauri::command]
pub fn rename_group(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: String,
    name: String,
) -> AppResult<()> {
    edit_vault(&app, &state, |v| v.rename_group(&group_id, &name))
}

#[tauri::command]
pub fn move_group(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: String,
    to: String,
) -> AppResult<()> {
    edit_vault(&app, &state, |v| v.move_group(&group_id, &to))
}

#[tauri::command]
pub fn move_entries(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_ids: Vec<String>,
    to: String,
) -> AppResult<()> {
    edit_vault(&app, &state, |v| v.move_entries(&entry_ids, &to))
}

// ---------------------------------------------------------------- attachments

/// Attachments larger than this are refused: the whole database lives in
/// memory, and so does every version of it in the history.
const MAX_ATTACHMENT: u64 = 256 * 1024 * 1024;

/// Saves an attachment to a file the user picks. The bytes go from the
/// database straight to the file, never through the UI. `false` when the
/// dialog was cancelled.
#[tauri::command]
pub async fn export_attachment(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    entry_id: String,
    name: String,
    text: DialogText,
) -> AppResult<bool> {
    // fail early (locked, no such attachment) before showing a dialog
    read(&state, |v| v.attachment(&entry_id, &name).map(|_| ()))?;
    let Some(path) = pick_save_path(app, window, text, &[], Some(name.clone()), None).await? else {
        return Ok(false);
    };
    let data = read(&state, |v| v.attachment(&entry_id, &name))?;
    std::fs::write(&path, &data[..]).map_err(|_| {
        log::warn!("an attachment could not be written to the chosen file");
        AppError::WriteFailed
    })?;
    Ok(true)
}

/// Attaches a file the user picks; returns the name it got (a taken name
/// gets a number), or `None` when the dialog was cancelled.
#[tauri::command]
pub async fn add_attachment(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    entry_id: String,
    text: DialogText,
) -> AppResult<Option<String>> {
    read(&state, |v| v.entry(&entry_id).map(|_| ()))?;
    let Some(path) = pick_file(app.clone(), window, text, &["*"], None).await? else {
        return Ok(None);
    };
    let size = std::fs::metadata(&path)
        .map_err(|_| AppError::FileUnreadable)?
        .len();
    if size > MAX_ATTACHMENT {
        return Err(AppError::TooLarge);
    }
    let data = zeroize::Zeroizing::new(std::fs::read(&path).map_err(|_| AppError::FileUnreadable)?);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "attachment".into());
    edit_vault(&app, &state, |v| v.add_attachment(&entry_id, &name, &data)).map(Some)
}

#[tauri::command]
pub fn remove_attachment(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
    name: String,
) -> AppResult<()> {
    edit_vault(&app, &state, |v| v.remove_attachment(&entry_id, &name))
}

// ---------------------------------------------------------------- new databases

/// Picks where a new database goes. `None` when cancelled.
#[tauri::command]
pub async fn choose_new_database(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    text: DialogText,
    file_name: String,
) -> AppResult<Option<String>> {
    let start = folder_of(session(&state)?.current.as_deref());
    let picked = pick_save_path(app, window, text, &["kdbx"], Some(file_name), start).await?;
    match picked {
        // a new database never replaces a file, even one the dialog agreed to
        Some(p) if p.exists() => Err(AppError::FileExists),
        Some(p) => Ok(Some(p.to_string_lossy().into_owned())),
        None => Ok(None),
    }
}

/// Makes a new key file where the user picks; returns its path, or `None`
/// when cancelled. Only the path comes back.
#[tauri::command]
pub async fn create_key_file(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    text: DialogText,
    file_name: String,
) -> AppResult<Option<String>> {
    let start = folder_of(session(&state)?.current.as_deref());
    let Some(path) = pick_save_path(app, window, text, &["keyx"], Some(file_name), start).await?
    else {
        return Ok(None);
    };
    mikoshi_core::vault::create_key_file(&path)?;
    log::info!("new key file written");
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Makes a new database at `path` (from `choose_new_database`), calibrates
/// its KDF to about a second here, writes it, and opens it. Whatever was
/// open is locked first (unsaved edits are saved, as when switching).
#[tauri::command]
pub async fn create_database(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: String,
    password: Option<Secret>,
    key_file: Option<String>,
) -> AppResult<VaultStatus> {
    let key_file = key_file.filter(|k| !k.trim().is_empty());
    let (file, kf) = (PathBuf::from(&path), key_file.clone());
    let vault = tauri::async_runtime::spawn_blocking(move || {
        let settings = mikoshi_core::vault::calibrated_settings()?;
        Vault::create(mikoshi_core::vault::NewDatabase {
            path: &file,
            name: &name,
            password: password.as_ref(),
            key_file: kf.as_deref().map(Path::new),
            settings,
        })
    })
    .await
    .map_err(|_| AppError::Internal)?
    .map_err(|e| {
        log::info!("new database not made: {e:?}");
        AppError::from(e)
    })?;
    log::info!("new database made");

    session::leave(&app, false);
    {
        let mut s = session(&state)?;
        s.lock();
        s.current = Some(PathBuf::from(&path));
        s.open = Some(vault);
    }
    let saved = settings(&state)?.update(|s| s.note_opened(&path, key_file.as_deref()));
    if let Ok(s) = saved {
        let _ = app.emit("settings-changed", &s);
    }
    announce(&app, &state)
}

// ---------------------------------------------------------------- generator

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedView {
    /// sent to the UI on purpose: it goes into the password field, or is
    /// shown so the user can copy it
    pub password: String,
    pub bits: f64,
    pub strength: Strength,
}

/// Generates a password and remembers the options (not the password).
#[tauri::command]
pub fn generate_password(
    app: AppHandle,
    state: State<'_, AppState>,
    options: GeneratorOptions,
) -> AppResult<GeneratedView> {
    let options = options.clamped();
    let g = generator::generate(&options).map_err(|_| AppError::NoCharacters)?;
    let changed = settings(&state)?.current.generator != options;
    if changed {
        // the lock must be released before emitting: the tray listens for
        // `settings-changed` on this thread and reads the settings itself
        let saved = settings(&state)?.update(|s| s.generator = options.clone());
        match saved {
            Ok(s) => {
                let _ = app.emit("settings-changed", &s);
            }
            Err(e) => log::warn!("generator options not saved: {e}"),
        }
    }
    Ok(GeneratedView {
        strength: generator::strength(g.password.expose()),
        password: g.password.expose().to_string(),
        bits: g.bits,
    })
}

/// How strong a password is (zxcvbn), for the editor's meter.
#[tauri::command]
pub fn password_strength(password: Secret) -> Strength {
    generator::strength(password.expose())
}

/// Copies a value the UI already shows (a generated password) as a secret:
/// kept out of clipboard history and cleared after the usual time.
#[tauri::command]
pub fn copy_secret(
    app: AppHandle,
    state: State<'_, AppState>,
    value: Secret,
) -> AppResult<CopyResult> {
    if value.is_empty() {
        return Err(AppError::NotFound);
    }
    let seconds = settings(&state)?.current.clipboard_clear_seconds;
    let clears_after = (seconds > 0).then_some(seconds);
    clipboard::copy(&app, &value, true, clears_after)?;
    Ok(CopyResult { clears_after })
}

// ---------------------------------------------------------------- settings: about

/// Something the "About" settings can open. The paths are decided here; the
/// UI only names which one.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum AppFile {
    /// the third-party licenses (installed next to the program)
    Notices,
    /// the folder with settings.json
    SettingsFolder,
    /// the folder with the logs
    LogFolder,
}

#[tauri::command]
pub fn open_app_file(app: AppHandle, which: AppFile) -> AppResult<()> {
    use tauri::Manager;
    let paths = app.path();
    let path = match which {
        AppFile::Notices => {
            let installed = paths
                .resource_dir()
                .map(|d| d.join("THIRD-PARTY-NOTICES.txt"))
                .ok()
                .filter(|p| p.is_file());
            // a development build runs from target\debug: use the repository's copy
            let dev = || {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("THIRD-PARTY-NOTICES.txt")
            };
            installed.unwrap_or_else(dev)
        }
        AppFile::SettingsFolder => paths.app_config_dir().map_err(|_| AppError::Internal)?,
        AppFile::LogFolder => paths.app_log_dir().map_err(|_| AppError::Internal)?,
    };
    if !path.exists() {
        return Err(AppError::FileNotFound);
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|_| {
            log::warn!("a file or folder could not be opened");
            AppError::Internal
        })
}

#[cfg(test)]
mod tests {
    use super::safe_url;

    #[test]
    fn opens_only_web_urls() {
        let ok = |s: &str| safe_url(s).map(|u| u.to_string());
        assert_eq!(
            ok("https://github.com/login").as_deref(),
            Some("https://github.com/login")
        );
        assert_eq!(
            ok("http://192.168.1.1").as_deref(),
            Some("http://192.168.1.1/")
        );
        assert_eq!(ok("  www.jd.com ").as_deref(), Some("https://www.jd.com/"));
        for bad in [
            "",
            "   ",
            "cmd://calc.exe",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "ftp://example.com",
            "https://exa\nmple.com",
            "https://",
        ] {
            assert!(safe_url(bad).is_none(), "{bad:?}");
        }
    }
}
