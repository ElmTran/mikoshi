//! New databases and new key files (features.md §3.1, §3.8; M8).
//!
//! A new database is KDBX 4.1, AES-256, Argon2id with 64 MiB of memory and
//! one lane per CPU core, its iterations calibrated so that unlocking takes
//! about a second here. It is written once, straight away, to a file that
//! must not exist yet: a new database never replaces anything.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::time::Duration;

use kdbx::{Cipher, Database, Key, Settings};
use zeroize::Zeroizing;

use super::{Vault, save};
use crate::Secret;

/// Argon2 memory for new databases: KeePassXC's default.
pub const NEW_KDF_MEMORY: u64 = 64 * 1024 * 1024;
/// How long unlocking a new database should take here.
pub const NEW_KDF_TARGET: Duration = Duration::from_secs(1);

/// Why a database could not be made. Carries no user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateError {
    /// there is already a file at that path
    Exists,
    /// neither a password nor a key file
    NoKey,
    KeyFileUnreadable,
    /// the KDF could not run (not enough memory, ...)
    Kdf,
    /// the file could not be written
    Write,
}

/// What a new database is made with.
pub struct NewDatabase<'a> {
    pub path: &'a Path,
    pub name: &'a str,
    pub password: Option<&'a Secret>,
    pub key_file: Option<&'a Path>,
    pub settings: Settings,
}

/// Argon2id calibrated for this machine (see the module docs).
pub fn calibrated_settings() -> Result<Settings, CreateError> {
    let lanes = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(2)
        .clamp(1, 16);
    let kdf = kdbx::calibrate_argon2id(NEW_KDF_MEMORY, lanes, NEW_KDF_TARGET)
        .map_err(|_| CreateError::Kdf)?;
    Ok(Settings {
        cipher: Cipher::Aes256,
        kdf,
        compressed: true,
    })
}

fn key_for(password: Option<&Secret>, key_file: Option<&Path>) -> Result<Key, CreateError> {
    let mut key = Key::new();
    if let Some(p) = password.filter(|p| !p.is_empty()) {
        key = key.with_password(p.expose());
    }
    if let Some(path) = key_file {
        let contents = Zeroizing::new(fs::read(path).map_err(|_| CreateError::KeyFileUnreadable)?);
        key = key
            .with_keyfile(&contents)
            .map_err(|_| CreateError::KeyFileUnreadable)?;
    }
    if key.is_empty() {
        return Err(CreateError::NoKey);
    }
    Ok(key)
}

/// Writes `bytes` to a file that must not exist yet, flushed to disk. A
/// failed write leaves nothing behind.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), CreateError> {
    let mut f = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(CreateError::Exists),
        Err(_) => return Err(CreateError::Write),
    };
    if f.write_all(bytes).and_then(|_| f.sync_all()).is_err() {
        drop(f);
        let _ = fs::remove_file(path);
        return Err(CreateError::Write);
    }
    Ok(())
}

impl Vault {
    /// Makes a new, empty database, writes it, and returns it unlocked.
    /// Runs the KDF once (to encrypt), so it takes about a second.
    pub fn create(new: NewDatabase<'_>) -> Result<Vault, CreateError> {
        if new.path.exists() {
            return Err(CreateError::Exists);
        }
        let key = key_for(new.password, new.key_file)?;
        let name = new.name.trim();
        let name = if name.is_empty() {
            super::file_stem(new.path)
        } else {
            name.to_string()
        };
        let db = Database::create(&name, &new.settings).map_err(|_| CreateError::Kdf)?;
        let bytes = db.save(&key).map_err(|_| CreateError::Kdf)?;
        write_new(new.path, &bytes)?;
        let (groups, group_index, entries) = Vault::layout(&db);
        Ok(Vault {
            db,
            path: new.path.to_path_buf(),
            key,
            disk: save::DiskState::of(&bytes),
            edits: 0,
            saved_edits: 0,
            // already KDBX 4.1: no format question on the first save
            saved: true,
            groups,
            group_index,
            entries,
        })
    }
}

/// A new key file in the XML 2.0 format KeePass 2.47+ and KeePassXC write:
/// 32 random bytes as hex, with the first 4 bytes of their SHA-256 as a
/// check. Written to a file that must not exist yet.
pub fn create_key_file(path: &Path) -> Result<(), CreateError> {
    use sha2::{Digest, Sha256};
    let key = Zeroizing::new({
        let mut k = [0u8; 32];
        getrandom::fill(&mut k).expect("the operating system's random number generator failed");
        k
    });
    let hex = Zeroizing::new(key.iter().map(|b| format!("{b:02X}")).collect::<String>());
    let groups: Vec<&str> = (0..8).map(|i| &hex[i * 8..i * 8 + 8]).collect();
    let hash: String = Sha256::digest(&key[..])[..4]
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    let text = Zeroizing::new(format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
         <KeyFile>\r\n\
         \t<Meta>\r\n\
         \t\t<Version>2.0</Version>\r\n\
         \t</Meta>\r\n\
         \t<Key>\r\n\
         \t\t<Data Hash=\"{hash}\">\r\n\
         \t\t\t{}\r\n\
         \t\t\t{}\r\n\
         \t\t</Data>\r\n\
         \t</Key>\r\n\
         </KeyFile>\r\n",
        groups[..4].join(" "),
        groups[4..].join(" "),
    ));
    write_new(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credentials;
    use crate::model::GroupKind;
    use kdbx::Kdf;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("mikoshi-create-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn quick() -> Settings {
        Settings {
            cipher: Cipher::Aes256,
            kdf: Kdf::Argon2 {
                id: true,
                memory: 1 << 20,
                iterations: 2,
                parallelism: 2,
                version: kdbx::ARGON2_VERSION,
            },
            compressed: true,
        }
    }

    #[test]
    fn a_new_database_is_written_and_opens_again() {
        let d = dir("db");
        let path = d.join("new.kdbx");
        let pw = Secret::new("new-master");
        let v = Vault::create(NewDatabase {
            path: &path,
            name: "My vault",
            password: Some(&pw),
            key_file: None,
            settings: quick(),
        })
        .unwrap();
        assert_eq!(v.name(), "My vault");
        assert!(!v.is_dirty());
        assert_eq!(v.format_upgrade(), None);
        assert!(v.groups().iter().any(|g| g.kind == GroupKind::Root));

        let again = Vault::open(
            &path,
            Credentials {
                password: Some(&pw),
                key_file: None,
            },
        )
        .unwrap();
        assert_eq!(again.name(), "My vault");

        // never over an existing file
        let err = Vault::create(NewDatabase {
            path: &path,
            name: "x",
            password: Some(&pw),
            key_file: None,
            settings: quick(),
        });
        assert_eq!(err.err(), Some(CreateError::Exists));
        let no_key = Vault::create(NewDatabase {
            path: &d.join("other.kdbx"),
            name: "x",
            password: None,
            key_file: None,
            settings: quick(),
        });
        assert_eq!(no_key.err(), Some(CreateError::NoKey));
        assert!(!d.join("other.kdbx").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_new_key_file_unlocks_a_new_database() {
        let d = dir("key");
        let key = d.join("new.keyx");
        create_key_file(&key).unwrap();
        assert_eq!(create_key_file(&key), Err(CreateError::Exists));
        let text = fs::read_to_string(&key).unwrap();
        assert!(text.contains("<Version>2.0</Version>"));

        let path = d.join("keyed.kdbx");
        let pw = Secret::new("pw");
        Vault::create(NewDatabase {
            path: &path,
            name: "",
            password: Some(&pw),
            key_file: Some(&key),
            settings: quick(),
        })
        .unwrap();
        let open = |kf: Option<&Path>| {
            Vault::open(
                &path,
                Credentials {
                    password: Some(&pw),
                    key_file: kf,
                },
            )
        };
        let v = open(Some(&key)).unwrap();
        assert_eq!(
            v.name(),
            "keyed",
            "an empty name falls back to the file name"
        );
        assert!(open(None).is_err(), "the key file is needed");

        // a damaged key file is refused, not silently used as raw bytes
        let broken = d.join("broken.keyx");
        let mut bad = text.clone();
        let i = bad.find("\t\t\t").unwrap() + 3;
        let flipped = if &bad[i..i + 1] == "0" { "1" } else { "0" };
        bad.replace_range(i..i + 1, flipped);
        fs::write(&broken, bad).unwrap();
        assert!(open(Some(&broken)).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    /// Writes a key file where `MIKOSHI_KEYFILE_OUT` says, for checking it
    /// with other programs (KeePassXC). Does nothing without the variable.
    #[test]
    #[ignore = "writes a file for a manual check; run with --ignored"]
    fn writes_a_key_file_for_other_programs() {
        if let Ok(path) = std::env::var("MIKOSHI_KEYFILE_OUT") {
            create_key_file(Path::new(&path)).unwrap();
        }
    }

    #[test]
    #[ignore = "takes about a second; run with --ignored"]
    fn calibration_lands_near_a_second() {
        let s = calibrated_settings().unwrap();
        let took = kdbx::time_kdf(&s.kdf).unwrap();
        eprintln!("calibrated {:?} -> {took:?}", s.kdf);
        assert!(
            took > Duration::from_millis(500) && took < Duration::from_secs(2),
            "{took:?}"
        );
    }
}
