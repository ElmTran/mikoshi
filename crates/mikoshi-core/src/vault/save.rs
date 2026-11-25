//! Writing the database back to its file (features.md §3.8, "保存").
//!
//! A save never leaves a half-written database behind:
//!
//! 1. The file must still be what was opened (or last saved): its content
//!    hash is compared. Someone else's change is a [`SaveError::Conflict`]
//!    unless the save is forced.
//! 2. `kdbx` encrypts the tree and reads its own output back.
//! 3. The bytes go to `NAME.mikoshi-tmp` next to the file, flushed to disk.
//! 4. `ReplaceFileW` swaps it in, moving the old file to `NAME.bak` (or to a
//!    scratch name that is deleted afterwards when backups are off). It keeps
//!    the old file's attributes and permissions. On failure the old file
//!    stays where it was and the temporary file is removed.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{OpenError, Vault};

/// What the file looked like when it was last read or written.
pub(super) struct DiskState {
    hash: [u8; 32],
}

impl DiskState {
    pub(super) fn of(data: &[u8]) -> DiskState {
        DiskState {
            hash: Sha256::digest(data).into(),
        }
    }
}

/// Why a save did not happen. Carries no user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveError {
    /// the file changed (or disappeared) since it was opened or last saved
    Conflict,
    /// the file is KDBX 3.1 or 4.0 and saving makes it 4.1: not agreed to yet
    FormatUpgrade,
    /// the database could not be encrypted, or did not read back
    Encrypt,
    /// the file could not be written (no permission, disk full, ...)
    Write,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOptions {
    /// overwrite even when the file changed on disk
    #[serde(default)]
    pub force: bool,
    /// the user agreed to the format becoming KDBX 4.1
    #[serde(default)]
    pub upgrade: bool,
    /// keep the previous file as `NAME.bak`
    #[serde(default)]
    pub backup: bool,
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// `NAME.bak` for `NAME` (`vault.kdbx` → `vault.kdbx.bak`).
pub fn backup_path(path: &Path) -> PathBuf {
    sibling(path, ".bak")
}

impl Vault {
    /// The format version the file had when it was opened.
    pub fn opened_version(&self) -> (u16, u16) {
        self.db.opened_version()
    }

    /// `Some(version)` when saving will change the file's format to KDBX 4.1
    /// (the file is 3.1 or 4.0 and has not been saved since it was opened).
    pub fn format_upgrade(&self) -> Option<(u16, u16)> {
        let v = self.db.opened_version();
        (!self.saved && v != (4, 1)).then_some(v)
    }

    /// Whether the file is no longer what was opened or last saved (also
    /// when it is gone or cannot be read).
    pub fn changed_on_disk(&self) -> bool {
        match fs::read(&self.path) {
            Ok(data) => DiskState::of(&data).hash != self.disk.hash,
            Err(_) => true,
        }
    }

    /// Writes the database to its file. Runs the KDF, so it takes about as
    /// long as unlocking. The app saves in three steps instead, so the vault
    /// is not held while the KDF runs: [`Vault::prepare_save`],
    /// [`SaveJob::run`], [`Vault::finish_save`].
    pub fn save(&mut self, opts: SaveOptions) -> Result<(), SaveError> {
        let job = self.prepare_save(opts)?;
        let saved = job.run()?;
        self.finish_save(saved);
        Ok(())
    }

    /// Checks that a save may go ahead (format agreed, file not changed
    /// elsewhere) and takes a copy of everything it needs. Quick: no KDF.
    pub fn prepare_save(&self, opts: SaveOptions) -> Result<SaveJob, SaveError> {
        if !opts.upgrade && self.format_upgrade().is_some() {
            return Err(SaveError::FormatUpgrade);
        }
        if !opts.force && self.changed_on_disk() {
            return Err(SaveError::Conflict);
        }
        Ok(SaveJob {
            db: self.db.clone(),
            key: self.key.clone(),
            path: self.path.clone(),
            backup: opts.backup,
            edits: self.edits,
        })
    }

    /// Records a finished save. Edits made while it ran stay unsaved.
    pub fn finish_save(&mut self, saved: Saved) {
        self.disk = saved.disk;
        self.saved_edits = saved.edits;
        self.saved = true;
    }

    /// Writes the database as a new file next to its own,
    /// `STEM.mikoshi-conflict-YYYYMMDD-HHMMSS.kdbx` (UTC), and returns its
    /// path. For an automatic save that found the file changed by someone
    /// else: neither version is lost. The vault stays unsaved.
    pub fn save_conflict_copy(&self) -> Result<PathBuf, SaveError> {
        let bytes = self.db.save(&self.key).map_err(|_| SaveError::Encrypt)?;
        let stem = super::file_stem(&self.path);
        let stamp: String = kdbx::Timestamp::now()
            .to_iso()
            .chars()
            .filter_map(|c| match c {
                'T' => Some('-'),
                c if c.is_ascii_digit() => Some(c),
                _ => None,
            })
            .collect();
        for n in 1..100 {
            let extra = if n == 1 {
                String::new()
            } else {
                format!("-{n}")
            };
            let path = self
                .path
                .with_file_name(format!("{stem}.mikoshi-conflict-{stamp}{extra}.kdbx"));
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path);
            let mut f = match file {
                Ok(f) => f,
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(SaveError::Write),
            };
            if f.write_all(&bytes).and_then(|_| f.sync_all()).is_err() {
                drop(f);
                let _ = fs::remove_file(&path);
                return Err(SaveError::Write);
            }
            return Ok(path);
        }
        Err(SaveError::Write)
    }

    /// Reads the file again with the same key, dropping unsaved edits.
    pub fn reload(&mut self) -> Result<(), OpenError> {
        let data = fs::read(&self.path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => OpenError::FileNotFound,
            _ => OpenError::FileUnreadable,
        })?;
        let fresh = Vault::open_with(&self.path, self.key.clone(), &data)?;
        *self = fresh;
        Ok(())
    }
}

/// A save in progress: a copy of the database as it was when the save began.
pub struct SaveJob {
    pub(super) db: kdbx::Database,
    pub(super) key: kdbx::Key,
    path: PathBuf,
    backup: bool,
    edits: u64,
}

impl std::fmt::Debug for SaveJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // never print the database or the key
        f.debug_struct("SaveJob")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// What a finished save hands back to the vault.
pub struct Saved {
    disk: DiskState,
    edits: u64,
}

impl SaveJob {
    /// The file this writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Encrypts (runs the KDF) and replaces the file. Needs no lock.
    pub fn run(self) -> Result<Saved, SaveError> {
        let bytes = self.db.save(&self.key).map_err(|_| SaveError::Encrypt)?;
        write_replacing(&self.path, &bytes, self.backup)?;
        Ok(Saved {
            disk: DiskState::of(&bytes),
            edits: self.edits,
        })
    }
}

/// Puts `bytes` in place of the file at `path` (see the module docs).
fn write_replacing(path: &Path, bytes: &[u8], backup: bool) -> Result<(), SaveError> {
    let tmp = sibling(path, ".mikoshi-tmp");
    let written = fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
        return Err(SaveError::Write);
    }
    let old = if backup {
        backup_path(path)
    } else {
        sibling(path, ".mikoshi-old")
    };
    match replace(path, &tmp, &old) {
        Ok(()) => {
            if !backup {
                let _ = fs::remove_file(&old);
            }
            Ok(())
        }
        Err(()) => {
            let _ = fs::remove_file(&tmp);
            Err(SaveError::Write)
        }
    }
}

#[cfg(windows)]
fn replace(path: &Path, tmp: &Path, old: &Path) -> Result<(), ()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_UNABLE_TO_MOVE_REPLACEMENT_2};
    use windows::Win32::Storage::FileSystem::{
        REPLACEFILE_IGNORE_ACL_ERRORS, REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW,
    };
    use windows::core::PCWSTR;

    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() };
    let (p, t, o) = (wide(path), wide(tmp), wide(old));
    // SAFETY: the three strings are NUL-terminated and outlive the call
    let r = unsafe {
        ReplaceFileW(
            PCWSTR(p.as_ptr()),
            PCWSTR(t.as_ptr()),
            PCWSTR(o.as_ptr()),
            REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS,
            None,
            None,
        )
    };
    match r {
        Ok(()) => Ok(()),
        // the file is gone (a forced save after someone deleted it)
        Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() && !path.exists() => {
            fs::rename(tmp, path).map_err(|_| ())
        }
        // the old file was moved to `old` but the new one could not take its
        // place: try once more, else put the old one back
        Err(e) if e.code() == ERROR_UNABLE_TO_MOVE_REPLACEMENT_2.to_hresult() => {
            if fs::rename(tmp, path).is_ok() {
                return Ok(());
            }
            let _ = fs::rename(old, path);
            Err(())
        }
        Err(_) => Err(()),
    }
}

#[cfg(not(windows))]
fn replace(path: &Path, tmp: &Path, old: &Path) -> Result<(), ()> {
    if path.exists() {
        fs::copy(path, old).map_err(|_| ())?;
    }
    fs::rename(tmp, path).map_err(|_| ())
}
