//! Database settings: the master key and the encryption settings (README,
//! "Roadmap"; KeePassXC's "Database settings → Security").
//!
//! A change is saved at once, and only takes effect in memory once the file
//! holds it: [`Vault::prepare_settings_save`] makes the save job from a copy
//! with the change applied, [`Vault::finish_settings_save`] applies it to the
//! vault after the job has written the file. Until then the vault keeps the
//! key the file on disk opens with, so a failed save, a conflict or a reload
//! never leaves it holding a key that does not match the file.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use kdbx::{Cipher, Kdf, Key, Timestamp};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::save::{SaveError, SaveJob, SaveOptions, Saved, backup_path};
use super::{Credentials, Vault};
use crate::Secret;

/// Argon2 memory limits offered (bytes): 1 MiB to 4 GiB.
pub const ARGON2_MEMORY_MIN: u64 = 1 << 20;
pub const ARGON2_MEMORY_MAX: u64 = 4 << 30;
/// Argon2 memory above which other devices (phones) may struggle.
pub const ARGON2_MEMORY_HEAVY: u64 = 256 << 20;
const ARGON2_ITERATIONS_MAX: u64 = 100_000;
const ARGON2_LANES_MAX: u32 = 64;
const AES_ROUNDS_MIN: u64 = 1_000;
const AES_ROUNDS_MAX: u64 = 10_000_000_000;

/// How long unlocking should take after "calibrate".
pub const CALIBRATE_TARGET: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CipherName {
    Aes256,
    ChaCha20,
    Twofish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KdfName {
    Argon2id,
    Argon2d,
    AesKdf,
}

/// The encryption settings as the settings page shows and changes them.
/// For AES-KDF `iterations` is its rounds; `memory` and `parallelism` are
/// only used by Argon2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Encryption {
    pub cipher: CipherName,
    pub kdf: KdfName,
    /// bytes
    pub memory: u64,
    pub iterations: u64,
    pub parallelism: u32,
    pub compressed: bool,
}

/// What the database settings page shows. No secrets.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseSettings {
    pub encryption: Encryption,
    pub has_password: bool,
    pub has_key_file: bool,
    /// the format the file has now ("4.1" once saved by Mikoshi)
    pub version: String,
    /// a `NAME.bak` lies next to the file
    pub has_backup: bool,
}

/// Why a settings change did not happen. Carries no user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    /// the current password (or key file) given to confirm is not the one
    WrongKey,
    /// the change would leave neither a password nor a key file
    NoKey,
    KeyFileUnreadable,
    /// a KDF parameter is out of range
    Invalid,
    /// the KDF could not run (not enough memory, ...)
    Kdf,
    Save(SaveError),
}

impl From<SaveError> for SettingsError {
    fn from(e: SaveError) -> Self {
        SettingsError::Save(e)
    }
}

pub enum PasswordChange<'a> {
    Keep,
    Remove,
    Set(&'a Secret),
}

pub enum KeyFileChange<'a> {
    Keep,
    Remove,
    Set(&'a Path),
}

/// A checked change, ready to be saved: the new key and/or settings, and
/// when it was made (the same time goes into the saved file and the vault).
pub struct SettingsChange {
    key: Option<Key>,
    settings: Option<kdbx::Settings>,
    at: Timestamp,
}

impl std::fmt::Debug for SettingsChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsChange")
            .field("key", &self.key.is_some())
            .field("settings", &self.settings.is_some())
            .finish()
    }
}

impl SettingsChange {
    pub fn changes_key(&self) -> bool {
        self.key.is_some()
    }
}

fn cipher_name(c: Cipher) -> CipherName {
    match c {
        Cipher::Aes256 => CipherName::Aes256,
        Cipher::ChaCha20 => CipherName::ChaCha20,
        Cipher::Twofish => CipherName::Twofish,
    }
}

fn cipher_of(c: CipherName) -> Cipher {
    match c {
        CipherName::Aes256 => Cipher::Aes256,
        CipherName::ChaCha20 => Cipher::ChaCha20,
        CipherName::Twofish => Cipher::Twofish,
    }
}

fn encryption_of(s: &kdbx::Settings) -> Encryption {
    let (kdf, memory, iterations, parallelism) = match s.kdf {
        Kdf::Aes { rounds } => (KdfName::AesKdf, 0, rounds, 0),
        Kdf::Argon2 {
            id,
            memory,
            iterations,
            parallelism,
            ..
        } => (
            if id {
                KdfName::Argon2id
            } else {
                KdfName::Argon2d
            },
            memory,
            iterations,
            parallelism,
        ),
    };
    Encryption {
        cipher: cipher_name(s.cipher),
        kdf,
        memory,
        iterations,
        parallelism,
        compressed: s.compressed,
    }
}

/// The `kdbx` settings for `e`, keeping the Argon2 version of `current` when
/// it stays Argon2. `Invalid` when a parameter is out of range.
fn settings_of(e: &Encryption, current: &Kdf) -> Result<kdbx::Settings, SettingsError> {
    let kdf = match e.kdf {
        KdfName::AesKdf => {
            if !(AES_ROUNDS_MIN..=AES_ROUNDS_MAX).contains(&e.iterations) {
                return Err(SettingsError::Invalid);
            }
            Kdf::Aes {
                rounds: e.iterations,
            }
        }
        KdfName::Argon2id | KdfName::Argon2d => {
            let ok = (ARGON2_MEMORY_MIN..=ARGON2_MEMORY_MAX).contains(&e.memory)
                && (1..=ARGON2_ITERATIONS_MAX).contains(&e.iterations)
                && (1..=ARGON2_LANES_MAX).contains(&e.parallelism)
                // Argon2 needs 8 KiB per lane at least
                && e.memory / 1024 >= 8 * u64::from(e.parallelism);
            if !ok {
                return Err(SettingsError::Invalid);
            }
            let version = match current {
                Kdf::Argon2 { version, .. } => *version,
                Kdf::Aes { .. } => kdbx::ARGON2_VERSION,
            };
            Kdf::Argon2 {
                id: e.kdf == KdfName::Argon2id,
                memory: e.memory,
                iterations: e.iterations,
                parallelism: e.parallelism,
                version,
            }
        }
    };
    Ok(kdbx::Settings {
        cipher: cipher_of(e.cipher),
        kdf,
        compressed: e.compressed,
    })
}

/// How long unlocking takes here with `e`. Runs the KDF once.
pub fn time_kdf(e: &Encryption) -> Result<Duration, SettingsError> {
    let s = settings_of(e, &Kdf::Aes { rounds: 0 })?;
    kdbx::time_kdf(&s.kdf).map_err(|_| SettingsError::Kdf)
}

/// `e` with its iterations (or rounds) chosen so unlocking takes about
/// [`CALIBRATE_TARGET`] here. Takes a few runs of the KDF.
pub fn calibrate(e: &Encryption) -> Result<Encryption, SettingsError> {
    let s = settings_of(e, &Kdf::Aes { rounds: 0 })?;
    let kdf = kdbx::calibrate(&s.kdf, CALIBRATE_TARGET).map_err(|_| SettingsError::Kdf)?;
    Ok(encryption_of(&kdbx::Settings { kdf, ..s }))
}

fn read_key_file(path: &Path) -> Result<Key, SettingsError> {
    let contents = Zeroizing::new(fs::read(path).map_err(|_| SettingsError::KeyFileUnreadable)?);
    Key::new()
        .with_keyfile(&contents)
        .map_err(|_| SettingsError::KeyFileUnreadable)
}

impl Vault {
    /// What the database settings page shows.
    pub fn database_settings(&self) -> DatabaseSettings {
        let (major, minor) = if self.saved {
            (4, 1)
        } else {
            self.db.opened_version()
        };
        DatabaseSettings {
            encryption: encryption_of(&self.db.settings()),
            has_password: self.key.has_password(),
            has_key_file: self.key.has_keyfile(),
            version: format!("{major}.{minor}"),
            has_backup: backup_path(&self.path).is_file(),
        }
    }

    /// Checks the current key, given again to confirm a change: the password
    /// when the database has one (a key file can be copied off the disk by
    /// whoever sits at the computer, a password cannot), else the key file.
    pub fn check_current_key(&self, current: Credentials<'_>) -> Result<(), SettingsError> {
        let ok = if self.key.has_password() {
            let typed = current
                .password
                .filter(|p| !p.is_empty())
                .ok_or(SettingsError::WrongKey)?;
            Key::new()
                .with_password(typed.expose())
                .same_password(&self.key)
        } else {
            let path = current.key_file.ok_or(SettingsError::WrongKey)?;
            read_key_file(path)?.same_keyfile(&self.key)
        };
        if ok {
            Ok(())
        } else {
            Err(SettingsError::WrongKey)
        }
    }

    /// Builds a change from what the page asks for. A new key needs the
    /// current one (see [`Vault::check_current_key`]); `None` keeps the key
    /// or the encryption settings as they are.
    pub fn settings_change(
        &self,
        current: Credentials<'_>,
        key: Option<(PasswordChange<'_>, KeyFileChange<'_>)>,
        encryption: Option<&Encryption>,
    ) -> Result<SettingsChange, SettingsError> {
        let key = match key {
            None => None,
            Some((password, key_file)) => {
                self.check_current_key(current)?;
                let base = match key_file {
                    KeyFileChange::Keep => Key::new().with_keyfile_of(&self.key),
                    KeyFileChange::Remove => Key::new(),
                    KeyFileChange::Set(path) => read_key_file(path)?,
                };
                let new = match password {
                    PasswordChange::Keep => base.with_password_of(&self.key),
                    PasswordChange::Remove => base,
                    // an empty password counts as none, as when unlocking
                    PasswordChange::Set(p) if p.is_empty() => base,
                    PasswordChange::Set(p) => base.with_password(p.expose()),
                };
                if new.is_empty() {
                    return Err(SettingsError::NoKey);
                }
                Some(new)
            }
        };
        let settings = encryption
            .map(|e| settings_of(e, &self.db.settings().kdf))
            .transpose()?;
        Ok(SettingsChange {
            key,
            settings,
            at: Timestamp::now(),
        })
    }

    fn apply_change(db: &mut kdbx::Database, key: &mut Key, change: &SettingsChange) {
        if let Some(k) = &change.key {
            *key = k.clone();
            db.note_master_key_changed(change.at);
        }
        if let Some(s) = &change.settings {
            db.set_settings(s);
            db.note_settings_changed(change.at);
        }
    }

    /// [`Vault::prepare_save`] for a copy with `change` applied. The vault
    /// itself is not changed (see the module docs).
    pub fn prepare_settings_save(
        &self,
        change: &SettingsChange,
        opts: SaveOptions,
    ) -> Result<SaveJob, SaveError> {
        let mut job = self.prepare_save(opts)?;
        Vault::apply_change(&mut job.db, &mut job.key, change);
        Ok(job)
    }

    /// Records a finished settings save and applies the change to the vault.
    pub fn finish_settings_save(&mut self, saved: Saved, change: &SettingsChange) {
        Vault::apply_change(&mut self.db, &mut self.key, change);
        self.finish_save(saved);
    }

    /// The whole settings save on this thread (tests, and callers that hold
    /// the vault anyway).
    pub fn save_settings(
        &mut self,
        change: &SettingsChange,
        opts: SaveOptions,
    ) -> Result<(), SaveError> {
        let job = self.prepare_settings_save(change, opts)?;
        let saved = job.run()?;
        self.finish_settings_save(saved, change);
        Ok(())
    }

    /// Deletes `NAME.bak` next to the database. `false` when there was none.
    pub fn remove_backup(&self) -> Result<bool, SaveError> {
        match fs::remove_file(backup_path(&self.path)) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(_) => Err(SaveError::Write),
        }
    }
}
