//! User preferences, stored as JSON in `%APPDATA%\com.elmtran.mikoshi\settings.json`.
//!
//! Only non-sensitive data belongs here (features.md §6): appearance, language,
//! and later recent file paths, key file paths and hotkeys. Never a password.

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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: ThemePref,
    pub language: LanguagePref,
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

        let s = Settings {
            theme: ThemePref::Dark,
            language: LanguagePref::EnUs,
        };
        store.save(s.clone()).unwrap();
        assert_eq!(Store::load(dir.clone()).current, s);

        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("\"theme\": \"dark\""));
        assert!(text.contains("\"language\": \"en-US\""));
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
        let _ = fs::remove_dir_all(&dir);
    }
}
