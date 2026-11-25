//! Database settings: changing the master key and the encryption settings,
//! against copies of the test databases in a temporary directory.

use super::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

const PW: &str = "mikoshi-test";

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(rel)
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mikoshi-dbs-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A fresh copy of the KDBX 4.1 password-only test database.
fn fresh(name: &str) -> (PathBuf, PathBuf, Vault) {
    let d = dir(name);
    let path = d.join("vault.kdbx");
    fs::copy(fixture("kxc/kdbx41-pw.kdbx"), &path).unwrap();
    let v = open(&path, Some(PW), None).unwrap();
    (d, path, v)
}

fn open(path: &Path, pw: Option<&str>, key_file: Option<&Path>) -> Result<Vault, OpenError> {
    let pw = pw.map(Secret::new);
    Vault::open(
        path,
        Credentials {
            password: pw.as_ref(),
            key_file,
        },
    )
}

fn with_pw(pw: &Secret) -> Credentials<'_> {
    Credentials {
        password: Some(pw),
        key_file: None,
    }
}

fn no_creds() -> Credentials<'static> {
    Credentials {
        password: None,
        key_file: None,
    }
}

/// Cheap settings, so the tests do not spend seconds in the KDF.
fn quick(cipher: CipherName, kdf: KdfName) -> Encryption {
    Encryption {
        cipher,
        kdf,
        memory: 1 << 20,
        iterations: if kdf == KdfName::AesKdf { 1_000 } else { 2 },
        parallelism: 2,
        compressed: true,
    }
}

fn master_key_changed(v: &Vault) -> String {
    v.db.meta()
        .element()
        .child_text("MasterKeyChanged")
        .unwrap_or_default()
        .into_owned()
}

fn opts() -> SaveOptions {
    SaveOptions {
        backup: true,
        ..Default::default()
    }
}

#[test]
fn the_page_sees_the_settings_and_no_secrets() {
    let (d, _, v) = fresh("view");
    let s = v.database_settings();
    assert!(s.has_password);
    assert!(!s.has_key_file);
    assert_eq!(s.version, "4.1");
    assert!(!s.has_backup);
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains(PW));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_new_password_needs_the_current_one() {
    let (d, path, mut v) = fresh("password");
    let new = Secret::new("brand new");
    let wrong = Secret::new("not it");

    for current in [with_pw(&wrong), no_creds()] {
        let err = v
            .settings_change(
                current,
                Some((PasswordChange::Set(&new), KeyFileChange::Keep)),
                None,
            )
            .unwrap_err();
        assert_eq!(err, SettingsError::WrongKey);
    }

    let current = Secret::new(PW);
    let before = master_key_changed(&v);
    let change = v
        .settings_change(
            with_pw(&current),
            Some((PasswordChange::Set(&new), KeyFileChange::Keep)),
            None,
        )
        .unwrap();
    assert!(change.changes_key());
    v.save_settings(&change, opts()).unwrap();
    assert!(!v.is_dirty());
    assert_ne!(master_key_changed(&v), before);

    assert_eq!(open(&path, Some(PW), None).err(), Some(OpenError::WrongKey));
    let again = open(&path, Some("brand new"), None).unwrap();
    assert_eq!(master_key_changed(&again), master_key_changed(&v));
    // the backup is the file before: it still opens with the old password
    let bak = backup_path(&path);
    assert!(open(&bak, Some(PW), None).is_ok());
    assert!(v.database_settings().has_backup);
    assert!(v.remove_backup().unwrap());
    assert!(!bak.exists());
    assert!(!v.remove_backup().unwrap());

    // the vault itself now holds the new key: reloading works, and the
    // next check wants the new password
    v.reload().unwrap();
    assert_eq!(
        v.check_current_key(with_pw(&current)),
        Err(SettingsError::WrongKey)
    );
    assert_eq!(v.check_current_key(with_pw(&new)), Ok(()));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_key_file_can_replace_the_password_and_be_removed_again() {
    let (d, path, mut v) = fresh("keyfile");
    let key_file = d.join("new.keyx");
    create_key_file(&key_file).unwrap();
    let current = Secret::new(PW);

    let change = v
        .settings_change(
            with_pw(&current),
            Some((PasswordChange::Remove, KeyFileChange::Set(&key_file))),
            None,
        )
        .unwrap();
    v.save_settings(&change, opts()).unwrap();
    let s = v.database_settings();
    assert!(!s.has_password && s.has_key_file);
    assert!(open(&path, None, Some(&key_file)).is_ok());
    assert!(open(&path, Some(PW), Some(&key_file)).is_err());

    // without a password, the key file confirms the change
    let other = d.join("other.keyx");
    create_key_file(&other).unwrap();
    fn by_file(p: &Path) -> Credentials<'_> {
        Credentials {
            password: None,
            key_file: Some(p),
        }
    }
    assert_eq!(
        v.check_current_key(by_file(&other)),
        Err(SettingsError::WrongKey)
    );
    assert_eq!(
        v.check_current_key(with_pw(&current)),
        Err(SettingsError::WrongKey)
    );

    // nothing left is refused
    let err = v
        .settings_change(
            by_file(&key_file),
            Some((PasswordChange::Remove, KeyFileChange::Remove)),
            None,
        )
        .unwrap_err();
    assert_eq!(err, SettingsError::NoKey);
    let empty = Secret::new("");
    let err = v
        .settings_change(
            by_file(&key_file),
            Some((PasswordChange::Set(&empty), KeyFileChange::Remove)),
            None,
        )
        .unwrap_err();
    assert_eq!(
        err,
        SettingsError::NoKey,
        "an empty password is no password"
    );

    // a password and the key file, then the password alone again
    let both = Secret::new("both");
    let change = v
        .settings_change(
            by_file(&key_file),
            Some((PasswordChange::Set(&both), KeyFileChange::Keep)),
            None,
        )
        .unwrap();
    v.save_settings(&change, opts()).unwrap();
    assert!(open(&path, Some("both"), Some(&key_file)).is_ok());
    assert!(open(&path, Some("both"), None).is_err());

    let change = v
        .settings_change(
            with_pw(&both),
            Some((PasswordChange::Keep, KeyFileChange::Remove)),
            None,
        )
        .unwrap();
    v.save_settings(&change, opts()).unwrap();
    assert!(open(&path, Some("both"), None).is_ok());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn every_cipher_and_kdf_saves_and_opens_again() {
    let (d, path, mut v) = fresh("encryption");
    let entries = v.search("").len();
    for cipher in [
        CipherName::Aes256,
        CipherName::ChaCha20,
        CipherName::Twofish,
    ] {
        for kdf in [KdfName::Argon2id, KdfName::Argon2d, KdfName::AesKdf] {
            let mut e = quick(cipher, kdf);
            e.compressed = cipher != CipherName::Twofish;
            // no key change: no current key needed
            let change = v.settings_change(no_creds(), None, Some(&e)).unwrap();
            assert!(!change.changes_key());
            v.save_settings(&change, opts()).unwrap();
            // AES-KDF has no memory or lanes: they read back as 0
            if kdf == KdfName::AesKdf {
                e.memory = 0;
                e.parallelism = 0;
            }
            assert_eq!(v.database_settings().encryption, e);
            let again = open(&path, Some(PW), None).unwrap();
            assert_eq!(
                again.database_settings().encryption,
                e,
                "{cipher:?} {kdf:?}"
            );
            assert_eq!(again.search("").len(), entries);
        }
    }
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn out_of_range_parameters_are_refused() {
    let (d, _, v) = fresh("invalid");
    let base = quick(CipherName::Aes256, KdfName::Argon2id);
    for bad in [
        Encryption {
            memory: 1024,
            ..base
        },
        Encryption {
            iterations: 0,
            ..base
        },
        Encryption {
            parallelism: 0,
            ..base
        },
        Encryption {
            kdf: KdfName::AesKdf,
            iterations: 10,
            ..base
        },
    ] {
        assert_eq!(
            v.settings_change(no_creds(), None, Some(&bad)).unwrap_err(),
            SettingsError::Invalid
        );
    }
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_change_that_cannot_be_saved_leaves_the_old_key() {
    let (d, path, mut v) = fresh("conflict");
    let current = Secret::new(PW);
    let new = Secret::new("brand new");
    let change = v
        .settings_change(
            with_pw(&current),
            Some((PasswordChange::Set(&new), KeyFileChange::Keep)),
            Some(&quick(CipherName::ChaCha20, KdfName::Argon2id)),
        )
        .unwrap();

    // someone else changes the file first
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&path, &bytes).unwrap();
    assert_eq!(v.save_settings(&change, opts()), Err(SaveError::Conflict));
    // nothing changed in the vault: the old password still confirms, the
    // old settings are still there
    assert_eq!(v.check_current_key(with_pw(&current)), Ok(()));
    assert_eq!(v.database_settings().encryption.cipher, CipherName::Aes256);

    // forced, it goes through
    v.save_settings(
        &change,
        SaveOptions {
            force: true,
            ..opts()
        },
    )
    .unwrap();
    assert!(open(&path, Some("brand new"), None).is_ok());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_kdbx_3_file_asks_before_it_becomes_4_1() {
    let d = dir("upgrade");
    let path = d.join("old.kdbx");
    fs::copy(fixture("kxc/kdbx31-pw.kdbx"), &path).unwrap();
    let mut v = open(&path, Some(PW), None).unwrap();
    assert_eq!(v.database_settings().version, "3.1");
    let e = quick(CipherName::Aes256, KdfName::Argon2id);
    let change = v.settings_change(no_creds(), None, Some(&e)).unwrap();
    assert_eq!(
        v.save_settings(&change, opts()),
        Err(SaveError::FormatUpgrade)
    );
    v.save_settings(
        &change,
        SaveOptions {
            upgrade: true,
            ..opts()
        },
    )
    .unwrap();
    assert_eq!(v.database_settings().version, "4.1");
    let again = open(&path, Some(PW), None).unwrap();
    assert_eq!(again.database_settings().encryption, e);
    let _ = fs::remove_dir_all(&d);
}

#[test]
#[ignore = "runs the KDF for a few seconds; run with --ignored"]
fn calibrating_lands_near_a_second() {
    for kdf in [KdfName::Argon2id, KdfName::AesKdf] {
        let e = calibrate(&Encryption {
            memory: 64 << 20,
            parallelism: 4,
            ..quick(CipherName::Aes256, kdf)
        })
        .unwrap();
        let took = time_kdf(&e).unwrap();
        eprintln!("{kdf:?}: {} -> {took:?}", e.iterations);
        assert!(
            took > Duration::from_millis(500) && took < Duration::from_secs(2),
            "{took:?}"
        );
    }
}
