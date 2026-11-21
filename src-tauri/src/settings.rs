//! User preferences, stored as JSON in `%APPDATA%\com.elmtran.mikoshi\settings.json`.
//!
//! Only non-sensitive data belongs here (features.md §6): appearance, language,
//! recent file paths, key file paths, list sorting, hotkeys. Never a
//! password, never the contents of a key file.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanguagePref {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

/// A database the user opened before, most recent first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFile {
    pub path: String,
    /// The key file last used with it (only the path). Kept only while
    /// `remember_key_files` is on.
    #[serde(default)]
    pub key_file: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    Title,
    Username,
    Url,
    #[default]
    Modified,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockUnsaved {
    /// save before locking (automatic locks always do)
    #[default]
    Save,
    /// ask on a manual lock; automatic locks save anyway
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EntrySort {
    pub key: SortKey,
    pub desc: bool,
}

impl Default for EntrySort {
    /// newest first (design/README.md)
    fn default() -> Self {
        EntrySort {
            key: SortKey::Modified,
            desc: true,
        }
    }
}

/// Shortcuts in the main window (features.md §5), in the hotkey text format
/// (see hotkeys.rs); "" = off. Only the UI reads them: they are handled in
/// the page, not registered with Windows. `Enter` to edit, `Ctrl+,` for the
/// settings and the global hotkeys are not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Shortcuts {
    pub search: String,
    pub copy_username: String,
    pub copy_password: String,
    pub copy_url: String,
    pub open_url: String,
    pub copy_totp: String,
    pub autotype: String,
    pub toggle_password: String,
    pub new_entry: String,
    pub edit_entry: String,
    pub delete_entry: String,
    pub save: String,
    pub lock: String,
    pub generator: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        let s = |t: &str| t.to_string();
        Shortcuts {
            search: s("Ctrl+F"),
            copy_username: s("Ctrl+B"),
            copy_password: s("Ctrl+C"),
            copy_url: s("Ctrl+U"),
            open_url: s("Ctrl+Shift+U"),
            copy_totp: s("Ctrl+T"),
            autotype: s("Ctrl+Shift+V"),
            toggle_password: s("Ctrl+H"),
            new_entry: s("Ctrl+N"),
            edit_entry: s("Ctrl+E"),
            delete_entry: s("Delete"),
            save: s("Ctrl+S"),
            lock: s("Ctrl+L"),
            generator: s("Ctrl+G"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: ThemePref,
    pub language: LanguagePref,
    /// Managed by the Rust side: the frontend cannot overwrite it through
    /// `set_settings`, only open or forget files.
    pub recent: Vec<RecentFile>,
    pub remember_key_files: bool,
    pub entry_sort: EntrySort,
    /// seconds until a copied secret leaves the clipboard; 0 = never
    pub clipboard_clear_seconds: u32,
    /// minutes without keyboard or mouse input (anywhere) before locking;
    /// 0 = never
    pub idle_lock_minutes: u32,
    pub lock_on_minimize: bool,
    /// global hotkey for the quick search box, like "Ctrl+Alt+K"; "" = off
    /// (format in hotkeys.rs)
    pub quick_search_hotkey: String,
    /// global hotkey that locks the database; "" = off
    pub lock_hotkey: String,
    /// global hotkey that auto-types into the window in front; "" = off
    pub autotype_hotkey: String,
    /// global hotkey that opens the quick search box as a form for a new
    /// entry, with the page in front filled in (quick save); "" = off
    pub quick_save_hotkey: String,
    /// the group quick save put the last entry in (its uuid). Managed by the
    /// Rust side, like `recent`. A database without that group uses its root.
    pub quick_save_group: Option<String>,
    /// milliseconds between auto-typed keys
    pub autotype_key_delay_ms: u32,
    /// keep the previous file as `NAME.bak` before each save
    pub backup_on_save: bool,
    /// write every change to the file at once (the UI saves after each
    /// edit); off: changes wait for Ctrl+S
    pub save_on_edit: bool,
    /// what happens to unsaved edits when the database is locked
    pub lock_unsaved: LockUnsaved,
    /// the password generator's last choices (never a generated password)
    pub generator: mikoshi_core::generator::GeneratorOptions,
    /// main-window shortcuts
    pub shortcuts: Shortcuts,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemePref::default(),
            language: LanguagePref::default(),
            recent: Vec::new(),
            remember_key_files: true,
            entry_sort: EntrySort::default(),
            // features.md §3.5 and §3.2
            clipboard_clear_seconds: 10,
            idle_lock_minutes: 5,
            lock_on_minimize: false,
            // features.md §3.6 and §5
            quick_search_hotkey: "Ctrl+Alt+K".into(),
            lock_hotkey: "Ctrl+Alt+L".into(),
            autotype_hotkey: "Ctrl+Alt+V".into(),
            quick_save_hotkey: "Ctrl+Alt+N".into(),
            quick_save_group: None,
            autotype_key_delay_ms: 20,
            backup_on_save: true,
            save_on_edit: true,
            lock_unsaved: LockUnsaved::default(),
            generator: Default::default(),
            shortcuts: Shortcuts::default(),
        }
    }
}

pub const MAX_RECENT: usize = 10;

/// Windows paths are case-insensitive.
fn same_path(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl Settings {
    /// Moves `path` to the top of the recent list, with the key file that
    /// opened it (if key files are remembered).
    pub fn note_opened(&mut self, path: &str, key_file: Option<&str>) {
        self.recent.retain(|r| !same_path(&r.path, path));
        self.recent.insert(
            0,
            RecentFile {
                path: path.to_string(),
                key_file: key_file
                    .filter(|_| self.remember_key_files)
                    .map(str::to_string),
            },
        );
        self.recent.truncate(MAX_RECENT);
    }

    pub fn forget(&mut self, path: &str) {
        self.recent.retain(|r| !same_path(&r.path, path));
    }

    pub fn key_file_for(&self, path: &str) -> Option<String> {
        self.recent
            .iter()
            .find(|r| same_path(&r.path, path))
            .and_then(|r| r.key_file.clone())
    }

    /// Takes the user-editable part of `incoming`; the recent list and the
    /// quick save group stay as they are here. Turning key file memory off
    /// forgets the stored paths.
    pub fn merge_from_ui(&self, incoming: Settings) -> Settings {
        let mut next = Settings {
            recent: self.recent.clone(),
            quick_save_group: self.quick_save_group.clone(),
            ..incoming
        };
        if !next.remember_key_files {
            for r in &mut next.recent {
                r.key_file = None;
            }
        }
        next
    }
}

pub struct Store {
    path: PathBuf,
    pub current: Settings,
}

impl Store {
    /// Loads settings from `dir`. A missing or unreadable file gives the
    /// defaults; an unreadable one is left in place until the next save.
    pub fn load(dir: PathBuf) -> Store {
        let path = dir.join("settings.json");
        let current = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("settings.json is not valid, using defaults: {e}");
                Settings::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            Err(e) => {
                log::warn!("cannot read settings.json, using defaults: {e}");
                Settings::default()
            }
        };
        Store { path, current }
    }

    /// Writes a temporary file next to the real one, then renames it over, so
    /// a crash mid-write never leaves a half-written settings file.
    pub fn save(&mut self, settings: Settings) -> AppResult<()> {
        let write = || -> std::io::Result<()> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            let tmp = self.path.with_extension("json.tmp");
            let mut file = fs::File::create(&tmp)?;
            file.write_all(serde_json::to_string_pretty(&settings)?.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, &self.path)
        };
        write().map_err(|e| {
            log::error!("cannot save settings: {e}");
            AppError::SettingsNotSaved
        })?;
        self.current = settings;
        Ok(())
    }

    /// Applies `change` to a copy of the current settings and saves it.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings)) -> AppResult<Settings> {
        let mut next = self.current.clone();
        change(&mut next);
        self.save(next.clone())?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mikoshi-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn round_trips_and_defaults() {
        let dir = temp_dir("settings");
        let mut store = Store::load(dir.clone());
        assert_eq!(store.current, Settings::default());
        assert!(store.current.remember_key_files);

        let mut s = Settings {
            theme: ThemePref::Dark,
            language: LanguagePref::EnUs,
            ..Settings::default()
        };
        s.note_opened(r"D:\a.kdbx", Some(r"D:\a.keyx"));
        store.save(s.clone()).unwrap();
        assert_eq!(Store::load(dir.clone()).current, s);

        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("\"theme\": \"dark\""));
        assert!(text.contains("\"language\": \"en-US\""));
        assert!(text.contains("\"keyFile\""));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tolerates_bad_and_partial_files() {
        let dir = temp_dir("settings-bad");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.json"), "{ not json").unwrap();
        assert_eq!(Store::load(dir.clone()).current, Settings::default());

        fs::write(
            dir.join("settings.json"),
            r#"{"theme":"light","futureField":1}"#,
        )
        .unwrap();
        let s = Store::load(dir.clone()).current;
        assert_eq!(s.theme, ThemePref::Light);
        assert_eq!(s.language, LanguagePref::System);
        assert!(s.remember_key_files);
        assert_eq!(s.entry_sort, EntrySort::default());
        assert_eq!(s.clipboard_clear_seconds, 10);
        assert_eq!(s.idle_lock_minutes, 5);
        assert_eq!(s.quick_search_hotkey, "Ctrl+Alt+K");
        assert_eq!(s.lock_hotkey, "Ctrl+Alt+L");
        assert_eq!(s.autotype_hotkey, "Ctrl+Alt+V");
        assert_eq!(s.quick_save_hotkey, "Ctrl+Alt+N");
        assert_eq!(s.quick_save_group, None);
        assert_eq!(s.autotype_key_delay_ms, 20);
        assert!(!s.lock_on_minimize);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_list_moves_to_front_and_is_capped() {
        let mut s = Settings::default();
        for i in 0..12 {
            s.note_opened(&format!(r"D:\{i}.kdbx"), None);
        }
        assert_eq!(s.recent.len(), MAX_RECENT);
        assert_eq!(s.recent[0].path, r"D:\11.kdbx");

        s.note_opened(r"d:\5.KDBX", Some(r"D:\k.key"));
        assert_eq!(s.recent[0].path, r"d:\5.KDBX");
        assert_eq!(
            s.recent
                .iter()
                .filter(|r| r.path.ends_with("5.kdbx"))
                .count(),
            0
        );
        assert_eq!(s.key_file_for(r"D:\5.kdbx").as_deref(), Some(r"D:\k.key"));

        s.forget(r"D:\5.kdbx");
        assert!(s.key_file_for(r"D:\5.kdbx").is_none());
    }

    #[test]
    fn ui_cannot_overwrite_recent_files_and_can_forget_key_files() {
        let mut s = Settings::default();
        s.note_opened(r"D:\a.kdbx", Some(r"D:\a.keyx"));
        s.quick_save_group = Some("g1".into());
        let stale = Settings {
            theme: ThemePref::Dark,
            ..Settings::default()
        };
        let merged = s.merge_from_ui(stale);
        assert_eq!(merged.theme, ThemePref::Dark);
        assert_eq!(merged.recent, s.recent);
        assert_eq!(merged.quick_save_group.as_deref(), Some("g1"));

        let off = Settings {
            remember_key_files: false,
            ..s.clone()
        };
        let merged = s.merge_from_ui(off);
        assert_eq!(merged.recent.len(), 1);
        assert!(merged.recent[0].key_file.is_none());

        let mut s = merged;
        s.note_opened(r"D:\b.kdbx", Some(r"D:\b.keyx"));
        assert!(s.key_file_for(r"D:\b.kdbx").is_none());
    }
}
