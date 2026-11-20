//! Editing and saving, against copies of the test databases in a temporary
//! directory: a save replaces the file it writes.

use super::edit::{AssociationEdit, AutotypeEdit, EntryEdit, FieldEdit, TotpEdit};
use super::*;
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(rel)
}

/// A fresh copy of `kxc/kdbx41-pw.kdbx` (or `rel`) in a new directory.
fn fresh(rel: &str) -> (PathBuf, Vault) {
    let dir = std::env::temp_dir().join(format!(
        "mikoshi-edit-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("vault.kdbx");
    fs::copy(fixture(rel), &path).unwrap();
    let pw = Secret::new("mikoshi-test");
    let v = Vault::open(
        &path,
        Credentials {
            password: Some(&pw),
            key_file: None,
        },
    )
    .unwrap();
    (dir, v)
}

fn edit_of(v: &Vault, id: &str) -> EntryEdit {
    let e = v.editable(id).unwrap();
    EntryEdit {
        title: e.title,
        username: e.username,
        password: Secret::new(e.password),
        url: e.url,
        notes: e.notes,
        tags: e.tags,
        expires: e.expires,
        fields: e
            .fields
            .into_iter()
            .map(|f| FieldEdit {
                original: Some(f.key.clone()),
                key: f.key,
                value: Secret::new(f.value),
                protected: f.protected,
            })
            .collect(),
        totp: TotpEdit::Unchanged,
        icon: None,
        autotype: None,
    }
}

fn new_edit(title: &str) -> EntryEdit {
    EntryEdit {
        title: title.into(),
        username: "user".into(),
        password: Secret::new("pa55"),
        url: "https://example.com".into(),
        notes: String::new(),
        tags: Vec::new(),
        expires: None,
        fields: Vec::new(),
        totp: TotpEdit::Unchanged,
        icon: None,
        autotype: None,
    }
}

fn first_entry(v: &Vault) -> String {
    v.search("").first().expect("an entry").id.clone()
}

fn reopened(v: &Vault) -> Vault {
    let pw = Secret::new("mikoshi-test");
    Vault::open(
        v.path(),
        Credentials {
            password: Some(&pw),
            key_file: None,
        },
    )
    .unwrap()
}

#[test]
fn an_edit_saves_and_reads_back() {
    let (dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let mut e = edit_of(&v, &id);
    e.title = "Edited title".into();
    e.password = Secret::new("a-new-secret-value");
    e.tags = vec!["x".into(), "y".into()];
    e.fields.push(FieldEdit {
        original: None,
        key: "PIN".into(),
        value: Secret::new("1234"),
        protected: true,
    });
    assert!(v.update_entry(&id, &e).unwrap());
    assert!(v.is_dirty());

    v.save(SaveOptions {
        backup: true,
        ..Default::default()
    })
    .unwrap();
    assert!(!v.is_dirty());

    let again = reopened(&v);
    let d = again.entry(&id).unwrap();
    assert_eq!(d.title, "Edited title");
    assert_eq!(d.tags, ["x", "y"]);
    let pin = d.fields.iter().find(|f| f.key == "PIN").unwrap();
    assert!(pin.protected && pin.value.is_none());
    assert_eq!(
        again.field(&id, "Password").unwrap().expose(),
        "a-new-secret-value"
    );
    assert_eq!(
        again.editable(&id).unwrap().fields.last().unwrap().value,
        "1234"
    );

    // backups and temporary files
    assert!(dir.join("vault.kdbx.bak").exists());
    assert!(!dir.join("vault.kdbx.mikoshi-tmp").exists());
    let backup = fs::read(dir.join("vault.kdbx.bak")).unwrap();
    assert_eq!(backup, fs::read(fixture("kxc/kdbx41-pw.kdbx")).unwrap());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_edit_that_changes_nothing_is_not_saved() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let e = edit_of(&v, &id);
    let history = v.history(&id).unwrap().len();
    assert!(!v.update_entry(&id, &e).unwrap());
    assert!(!v.is_dirty());
    assert_eq!(v.history(&id).unwrap().len(), history);
}

#[test]
fn editing_fills_in_the_history_once_per_edit() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let n = v.history(&id).unwrap().len();
    for i in 0..3 {
        let mut e = edit_of(&v, &id);
        e.title = format!("T{i}");
        assert!(v.update_entry(&id, &e).unwrap());
    }
    let h = v.history(&id).unwrap();
    assert_eq!(h.len(), n + 3);
    // each edit files away the version it replaced
    let titles: Vec<&str> = h[n..].iter().map(|x| x.title.as_str()).collect();
    assert_eq!(titles, ["With History", "T0", "T1"]);
    assert_eq!(v.entry(&id).unwrap().title, "T2");
    assert!(h[n + 2].modified.is_some());
    let _ = v;
}

#[test]
fn field_names_are_checked() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let mut e = edit_of(&v, &id);
    let same = |f: &mut FieldEdit| {
        f.original = None;
    };
    for bad in ["", " Title", "Title", "otp", "TimeOtp-Secret-Base32"] {
        let mut f = FieldEdit {
            original: None,
            key: bad.into(),
            value: Secret::new("v"),
            protected: false,
        };
        same(&mut f);
        e.fields.push(f);
        assert_eq!(v.update_entry(&id, &e), Err(EditError::FieldName), "{bad}");
        e.fields.pop();
    }
    for _ in 0..2 {
        e.fields.push(FieldEdit {
            original: None,
            key: "dupe".into(),
            value: Secret::new("v"),
            protected: false,
        });
    }
    assert_eq!(v.update_entry(&id, &e), Err(EditError::FieldName));
    assert!(!v.is_dirty());
}

#[test]
fn fields_can_be_renamed_swapped_and_removed() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let mut e = edit_of(&v, &id);
    // two new fields, then swap their names
    for k in ["A", "B"] {
        e.fields.push(FieldEdit {
            original: None,
            key: k.into(),
            value: Secret::new(k),
            protected: false,
        });
    }
    v.update_entry(&id, &e).unwrap();
    let mut e = edit_of(&v, &id);
    for f in &mut e.fields {
        match f.key.as_str() {
            "A" => f.key = "B".into(),
            "B" => f.key = "A".into(),
            _ => {}
        }
    }
    assert!(v.update_entry(&id, &e).unwrap());
    let d = v.entry(&id).unwrap();
    let val = |k: &str| d.fields.iter().find(|f| f.key == k).unwrap().value.clone();
    assert_eq!(val("A"), Some("B".into()));
    assert_eq!(val("B"), Some("A".into()));
    assert_eq!(d.fields.len(), e.fields.len());

    // removing a field the entry does not have is an error
    let mut bad = e;
    bad.fields.push(FieldEdit {
        original: Some("no such field".into()),
        key: "x".into(),
        value: Secret::new(""),
        protected: false,
    });
    assert_eq!(v.update_entry(&id, &bad), Err(EditError::NotFound));

    // dropping a field removes it
    let mut e = edit_of(&v, &id);
    let all = e.fields.len();
    e.fields.retain(|f| f.key != "A");
    assert!(v.update_entry(&id, &e).unwrap());
    let d = v.entry(&id).unwrap();
    assert_eq!(d.fields.len(), all - 1);
    assert!(d.fields.iter().all(|f| f.key != "A"));
    let _ = v;
}

#[test]
fn creating_entries_and_totp() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let root = v
        .groups()
        .into_iter()
        .find(|g| g.kind == GroupKind::Root)
        .unwrap();
    let mut e = new_edit("Generated");
    e.totp = TotpEdit::Manual {
        secret: Secret::new("jbsw y3dp ehpk 3pxp"),
        algorithm: "SHA256".into(),
        digits: 8,
        period: 60,
    };
    let id = v.create_entry(&root.id, &e).unwrap();
    assert!(v.is_dirty());
    let d = v.entry(&id).unwrap();
    assert_eq!(d.title, "Generated");
    assert_eq!(d.group_path.last().unwrap(), &root.name);
    assert!(d.has_totp);
    let editable = v.editable(&id).unwrap();
    let t = editable.totp.clone().unwrap();
    assert!(t.valid);
    assert_eq!(
        (t.algorithm, t.digits, t.period),
        (Some("SHA256"), Some(8), Some(60))
    );
    assert!(v.totp(&id).unwrap().is_ok());
    // the secret itself is never in the editable view
    let json = serde_json::to_string(&editable).unwrap();
    assert!(!json.contains("JBSWY3DPEHPK3PXP"));

    // pasted URI, then removing it
    let mut e = edit_of(&v, &id);
    e.totp = TotpEdit::Uri {
        uri: Secret::new("otpauth://totp/Example:alice?secret=JBSWY3DPEHPK3PXP&digits=6"),
    };
    assert!(v.update_entry(&id, &e).unwrap());
    assert_eq!(v.editable(&id).unwrap().totp.unwrap().digits, Some(6));
    let mut e = edit_of(&v, &id);
    e.totp = TotpEdit::Remove;
    assert!(v.update_entry(&id, &e).unwrap());
    assert!(!v.entry(&id).unwrap().has_totp);
    assert!(v.editable(&id).unwrap().totp.is_none());

    // bad settings are refused, and nothing changes
    let mut e = edit_of(&v, &id);
    e.totp = TotpEdit::Uri {
        uri: Secret::new("otpauth://hotp/x?secret=AAAA"),
    };
    assert_eq!(v.update_entry(&id, &e), Err(EditError::TotpInvalid));
    let mut e = edit_of(&v, &id);
    e.totp = TotpEdit::Manual {
        secret: Secret::new("not base32 1"),
        algorithm: "SHA1".into(),
        digits: 6,
        period: 30,
    };
    assert_eq!(v.update_entry(&id, &e), Err(EditError::TotpInvalid));

    // an entry cannot be created in the recycle bin
    let bin = v
        .groups()
        .into_iter()
        .find(|g| g.kind == GroupKind::RecycleBin);
    if let Some(bin) = bin {
        assert_eq!(
            v.create_entry(&bin.id, &new_edit("x")),
            Err(EditError::RecycleBin)
        );
    }
    let _ = v;
}

#[test]
fn a_totp_in_the_keepass_2_form_is_replaced_not_duplicated() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    // an entry with TimeOtp fields: the fixture has TOTP entries
    let id = v.search("").into_iter().find(|e| e.has_totp).unwrap().id;
    let mut e = edit_of(&v, &id);
    e.totp = TotpEdit::Manual {
        secret: Secret::new("JBSWY3DPEHPK3PXP"),
        algorithm: "SHA1".into(),
        digits: 6,
        period: 30,
    };
    assert!(v.update_entry(&id, &e).unwrap());
    let uuid = v.entries.get(&id).unwrap().0;
    let keys: Vec<String> =
        v.db.entry(uuid)
            .unwrap()
            .fields()
            .map(|f| f.key().into_owned())
            .collect();
    assert_eq!(
        keys.iter().filter(|k| is_otp_field(k)).count(),
        1,
        "{keys:?}"
    );
    assert!(keys.contains(&"otp".to_string()));
}

#[test]
fn history_versions_can_be_shown_and_restored() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let before = v.entry(&id).unwrap();
    let mut e = edit_of(&v, &id);
    e.title = "After".into();
    v.update_entry(&id, &e).unwrap();
    let n = v.history(&id).unwrap().len();
    assert!(n >= 1);
    let h = v.history_entry(&id, n - 1).unwrap();
    assert_eq!(h.title, before.title);
    assert!(
        v.history(&id)
            .unwrap()
            .iter()
            .all(|x| v.entry(&id).map(|d| d.title != x.title).unwrap_or(true))
    );
    let pw = v.history_field(&id, n - 1, "Password").unwrap();
    assert!(v.history_field(&id, n - 1, "otp").is_none());

    v.restore_history(&id, n - 1).unwrap();
    assert!(v.is_dirty());
    let d = v.entry(&id).unwrap();
    assert_eq!(d.title, before.title);
    assert_eq!(v.field(&id, "Password").unwrap().expose(), pw.expose());
    assert_eq!(v.history(&id).unwrap().len(), n + 1);
    assert!(v.restore_history(&id, 999).is_err());
    let _ = v;
}

#[test]
fn saving_detects_an_outside_change() {
    let (dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let mut e = edit_of(&v, &id);
    e.title = "Mine".into();
    v.update_entry(&id, &e).unwrap();
    // someone else writes the file
    let outside = fs::read(fixture("kxc/kdbx41-pw.kdbx")).unwrap();
    let mut changed = outside.clone();
    changed.push(0);
    fs::write(dir.join("vault.kdbx"), &changed).unwrap();
    assert!(v.changed_on_disk());
    assert_eq!(v.save(SaveOptions::default()), Err(SaveError::Conflict));
    assert!(v.is_dirty());
    assert_eq!(
        fs::read(dir.join("vault.kdbx")).unwrap(),
        changed,
        "untouched"
    );

    // forced: overwrites
    v.save(SaveOptions {
        force: true,
        backup: true,
        ..Default::default()
    })
    .unwrap();
    assert!(!v.is_dirty());
    assert!(!v.changed_on_disk());
    // the backup is the file that was there
    assert_eq!(fs::read(dir.join("vault.kdbx.bak")).unwrap(), changed);

    // and now saving again without a change on disk works
    v.save(SaveOptions::default()).unwrap();
    assert!(!dir.join("vault.kdbx.bak.mikoshi-tmp").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_second_save_succeeds_after_the_first() {
    let (dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    for i in 0..2 {
        let mut e = edit_of(&v, &id);
        e.notes = format!("note {i}");
        v.update_entry(&id, &e).unwrap();
        v.save(SaveOptions::default()).unwrap();
    }
    let again = reopened(&v);
    assert_eq!(again.entry(&id).unwrap().notes, "note 1");
    assert!(!dir.join("vault.kdbx.bak").exists(), "backups off");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_conflict_copy_leaves_both_versions() {
    let (dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let mut e = edit_of(&v, &id);
    e.title = "Autosaved".into();
    v.update_entry(&id, &e).unwrap();
    let saved = v.save_conflict_copy().unwrap();
    assert_eq!(saved.parent(), Some(dir.as_path()));
    assert!(
        saved
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(".mikoshi-conflict-")
    );
    assert!(v.is_dirty(), "the vault still has the unsaved edit");
    // the copy opens with the same key and has the edit
    let pw = Secret::new("mikoshi-test");
    let copied = Vault::open(
        &saved,
        Credentials {
            password: Some(&pw),
            key_file: None,
        },
    )
    .unwrap();
    assert_eq!(copied.entry(&id).unwrap().title, "Autosaved");
    // a second copy gets another name
    let two = v.save_conflict_copy().unwrap();
    assert_ne!(two, saved);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reloading_drops_unsaved_edits() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let title = v.entry(&id).unwrap().title;
    let mut e = edit_of(&v, &id);
    e.title = "Unsaved".into();
    v.update_entry(&id, &e).unwrap();
    v.reload().unwrap();
    assert!(!v.is_dirty());
    assert_eq!(v.entry(&id).unwrap().title, title);
    // the key still works after a reload
    let mut e = edit_of(&v, &id);
    e.title = "Saved after reload".into();
    v.update_entry(&id, &e).unwrap();
    assert!(v.save(SaveOptions::default()).is_ok());
    let _ = v;
}

#[test]
fn an_upgrade_needs_agreement_and_keeps_the_settings() {
    for rel in ["kxc/kdbx31-pw.kdbx", "kxc/kdbx40-pw.kdbx"] {
        let (_dir, mut v) = fresh(rel);
        assert_eq!(
            v.format_upgrade().map(|(a, _)| a),
            Some(if rel.contains("31") { 3 } else { 4 })
        );
        let settings = v.db.settings();
        assert_eq!(
            v.save(SaveOptions::default()),
            Err(SaveError::FormatUpgrade)
        );
        v.save(SaveOptions {
            upgrade: true,
            backup: true,
            ..Default::default()
        })
        .unwrap_or_else(|e| panic!("{rel}: {e:?}"));
        assert_eq!(v.format_upgrade(), None);

        assert_eq!(v.db.settings(), settings);
        // and the file is KDBX 4.1 now
        let data = fs::read(v.path()).unwrap();
        assert_eq!(kdbx::version(&data).unwrap(), (4, 1));
    }
}

#[test]
fn a_failed_save_leaves_no_temporary_file() {
    let (dir, v) = fresh("kxc/kdbx41-pw.kdbx");
    // make the directory read-only is awkward on Windows; instead point the
    // vault at a path whose directory is gone
    let missing = dir.join("gone").join("vault.kdbx");
    let mut broken =
        Vault::open_with(&missing, v.key.clone(), &fs::read(v.path()).unwrap()).unwrap();
    let id = first_entry(&broken);
    let mut e = edit_of(&broken, &id);
    e.title = "x".into();
    broken.update_entry(&id, &e).unwrap();
    // the file is gone, so without `force` this is a conflict; forced, the
    // write itself fails
    assert_eq!(
        broken.save(SaveOptions::default()),
        Err(SaveError::Conflict)
    );
    assert_eq!(
        broken.save(SaveOptions {
            force: true,
            ..Default::default()
        }),
        Err(SaveError::Write)
    );
    assert!(!dir.join("gone").join("vault.kdbx.mikoshi-tmp").exists());
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- M8: organizing

fn group_id(v: &Vault, name: &str) -> String {
    v.groups()
        .into_iter()
        .find(|g| g.name == name)
        .unwrap_or_else(|| panic!("no group {name}"))
        .id
}

fn bin_id(v: &Vault) -> String {
    v.groups()
        .into_iter()
        .find(|g| g.kind == GroupKind::RecycleBin)
        .expect("a recycle bin")
        .id
}

#[test]
fn deleting_goes_through_the_bin_then_for_good() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let from = v.entry(&id).unwrap().group_id;
    assert!(!v.deletes_permanently(std::slice::from_ref(&id), &[]));
    assert_eq!(
        v.delete_entries(std::slice::from_ref(&id), "Recycle Bin")
            .unwrap(),
        Deleted::ToRecycleBin
    );
    let bin = bin_id(&v);
    assert_eq!(v.entry(&id).unwrap().group_id, bin);
    assert!(
        v.search("").iter().all(|e| e.id != id),
        "the bin is not searched"
    );
    assert!(v.is_dirty());

    // back to where it came from
    v.restore(&id).unwrap();
    assert_eq!(v.entry(&id).unwrap().group_id, from);
    assert_eq!(v.restore(&id), Err(EditError::NotAllowed), "not in the bin");

    // in the bin, deleting is for good
    v.delete_entries(std::slice::from_ref(&id), "Recycle Bin")
        .unwrap();
    assert!(v.deletes_permanently(std::slice::from_ref(&id), &[]));
    assert_eq!(
        v.delete_entries(std::slice::from_ref(&id), "Recycle Bin")
            .unwrap(),
        Deleted::Permanently
    );
    assert!(v.entry(&id).is_none());

    v.save(SaveOptions::default()).unwrap();
    let again = reopened(&v);
    assert!(again.entry(&id).is_none());
}

#[test]
fn groups_are_made_renamed_moved_and_deleted() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let general = group_id(&v, "General");
    let root = v
        .groups()
        .into_iter()
        .find(|g| g.kind == GroupKind::Root)
        .unwrap()
        .id;
    assert_eq!(v.create_group(&general, "  "), Err(EditError::EmptyName));
    let sub = v.create_group(&general, "Sub").unwrap();
    v.rename_group(&sub, "Renamed").unwrap();
    let g = v.groups().into_iter().find(|g| g.id == sub).unwrap();
    assert_eq!(
        (g.name.as_str(), g.parent_id.as_deref()),
        ("Renamed", Some(general.as_str()))
    );

    // not into itself, not the root, not into the bin
    assert_eq!(v.move_group(&general, &sub), Err(EditError::NotAllowed));
    assert_eq!(v.move_group(&root, &general), Err(EditError::NotAllowed));
    let bin = bin_id(&v);
    assert_eq!(v.move_group(&sub, &bin), Err(EditError::NotAllowed));
    assert_eq!(
        v.delete_group(&root, "Recycle Bin"),
        Err(EditError::NotAllowed)
    );

    v.move_group(&sub, &root).unwrap();
    let id = first_entry(&v);
    v.move_entries(std::slice::from_ref(&id), &sub).unwrap();
    assert_eq!(v.entry(&id).unwrap().group_id, sub);

    // a deleted group goes to the bin with its entries, and comes back
    assert_eq!(
        v.delete_group(&sub, "Recycle Bin").unwrap(),
        Deleted::ToRecycleBin
    );
    let g = v.groups().into_iter().find(|g| g.id == sub).unwrap();
    assert_eq!(g.parent_id.as_deref(), Some(bin.as_str()));
    assert!(v.search("").iter().all(|e| e.id != id));
    v.restore(&sub).unwrap();
    let g = v.groups().into_iter().find(|g| g.id == sub).unwrap();
    assert_eq!(g.parent_id.as_deref(), Some(root.as_str()));
    assert!(v.search("").iter().any(|e| e.id == id));

    // emptying the bin
    v.delete_group(&sub, "Recycle Bin").unwrap();
    assert!(v.empty_recycle_bin().unwrap() >= 1);
    assert!(v.groups().iter().all(|g| g.id != sub));
    assert!(v.entry(&id).is_none());
    assert_eq!(v.empty_recycle_bin().unwrap(), 0);
}

#[test]
fn attachments_are_added_exported_and_removed() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let n = v.history(&id).unwrap().len();
    let name = v.add_attachment(&id, "note.txt", b"hello").unwrap();
    assert_eq!(name, "note.txt");
    let again = v.add_attachment(&id, "note.txt", b"second").unwrap();
    assert_eq!(again, "note (2).txt");
    assert_eq!(v.attachment(&id, "note.txt").unwrap().as_slice(), b"hello");
    assert_eq!(
        v.history(&id).unwrap().len(),
        n + 2,
        "each change keeps the old version"
    );

    v.remove_attachment(&id, "note.txt").unwrap();
    assert!(v.attachment(&id, "note.txt").is_none());
    assert_eq!(
        v.remove_attachment(&id, "note.txt"),
        Err(EditError::NotFound)
    );
    assert_eq!(
        v.attachment(&id, "note (2).txt").unwrap().as_slice(),
        b"second"
    );

    v.save(SaveOptions::default()).unwrap();
    let r = reopened(&v);
    let names: Vec<String> = r
        .entry(&id)
        .unwrap()
        .attachments
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert!(names.contains(&"note (2).txt".to_string()));
    assert!(!names.contains(&"note.txt".to_string()));
}

#[test]
fn a_standard_icon_can_be_picked() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = v
        .search("Custom Icon")
        .into_iter()
        .find(|e| e.custom_icon.is_some())
        .unwrap()
        .id;
    let mut e = edit_of(&v, &id);
    e.icon = Some(5);
    assert!(v.update_entry(&id, &e).unwrap());
    let d = v.entry(&id).unwrap();
    assert_eq!((d.icon, d.custom_icon), (5, None));
    // the same icon again changes nothing
    let mut e = edit_of(&v, &id);
    e.icon = Some(5);
    assert!(!v.update_entry(&id, &e).unwrap());
}

#[test]
fn an_edit_made_while_saving_stays_unsaved() {
    let (_dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let mut e = edit_of(&v, &id);
    e.title = "First".into();
    v.update_entry(&id, &e).unwrap();

    // the save copies the database, then runs without the vault
    let job = v.prepare_save(SaveOptions::default()).unwrap();
    let mut e = edit_of(&v, &id);
    e.title = "Second".into();
    v.update_entry(&id, &e).unwrap();
    let saved = job.run().unwrap();
    v.finish_save(saved);
    assert!(v.is_dirty(), "the second edit is not in the file yet");
    assert_eq!(reopened(&v).entry(&id).unwrap().title, "First");

    v.save(SaveOptions::default()).unwrap();
    assert!(!v.is_dirty());
    assert_eq!(reopened(&v).entry(&id).unwrap().title, "Second");
}

#[test]
fn autotype_settings_save_and_read_back() {
    let (dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let before = v.editable(&id).unwrap().autotype;
    assert!(before.enabled);
    assert_eq!(before.inherited, "{USERNAME}{TAB}{PASSWORD}{ENTER}");

    let mut e = edit_of(&v, &id);
    e.autotype = Some(AutotypeEdit {
        enabled: true,
        sequence: "{USERNAME}{TAB}{TAB}{PASSWORD}{ENTER}".into(),
        associations: vec![
            AssociationEdit {
                window: "*Example*".into(),
                sequence: String::new(),
            },
            // dropped: no window
            AssociationEdit {
                window: "  ".into(),
                sequence: "{PASSWORD}".into(),
            },
            AssociationEdit {
                window: "//Login - .*//".into(),
                sequence: "{CLEARFIELD}{PASSWORD}{ENTER}".into(),
            },
        ],
    });
    assert!(v.update_entry(&id, &e).unwrap());
    // the same settings again change nothing
    assert!(!v.update_entry(&id, &e).unwrap());
    v.save(SaveOptions::default()).unwrap();

    let again = reopened(&v);
    let at = again.editable(&id).unwrap().autotype;
    assert!(at.enabled);
    assert_eq!(at.sequence, "{USERNAME}{TAB}{TAB}{PASSWORD}{ENTER}");
    assert_eq!(
        at.associations,
        [
            AssociationEdit {
                window: "*Example*".into(),
                sequence: String::new(),
            },
            AssociationEdit {
                window: "//Login - .*//".into(),
                sequence: "{CLEARFIELD}{PASSWORD}{ENTER}".into(),
            },
        ]
    );
    // what auto-type will type
    assert_eq!(
        again
            .autotype_sequence(&id, "Example login")
            .unwrap()
            .unwrap(),
        "{USERNAME}{TAB}{TAB}{PASSWORD}{ENTER}"
    );
    assert_eq!(
        again
            .autotype_sequence(&id, "Login - site")
            .unwrap()
            .unwrap(),
        "{CLEARFIELD}{PASSWORD}{ENTER}"
    );

    // back to inheriting, no associations, turned off
    let mut v = again;
    let mut e = edit_of(&v, &id);
    e.autotype = Some(AutotypeEdit {
        enabled: false,
        sequence: String::new(),
        associations: Vec::new(),
    });
    assert!(v.update_entry(&id, &e).unwrap());
    let at = v.editable(&id).unwrap().autotype;
    assert!(!at.enabled);
    assert_eq!(at.sequence, "");
    assert!(at.associations.is_empty());
    assert!(v.autotype_sequence(&id, "anything").unwrap().is_err());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_sequence_that_cannot_be_typed_is_refused() {
    let (dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    for (sequence, window_sequence) in [("{NOPE}", ""), ("", "{USERNAME")] {
        let mut e = edit_of(&v, &id);
        e.autotype = Some(AutotypeEdit {
            enabled: true,
            sequence: sequence.into(),
            associations: vec![AssociationEdit {
                window: "*x*".into(),
                sequence: window_sequence.into(),
            }],
        });
        assert_eq!(v.update_entry(&id, &e), Err(EditError::AutotypeSequence));
    }
    assert!(!v.is_dirty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn leaving_autotype_out_keeps_it() {
    let (dir, mut v) = fresh("kxc/kdbx41-pw.kdbx");
    let id = first_entry(&v);
    let mut e = edit_of(&v, &id);
    e.autotype = Some(AutotypeEdit {
        enabled: true,
        sequence: "{PASSWORD}{ENTER}".into(),
        associations: Vec::new(),
    });
    v.update_entry(&id, &e).unwrap();
    let mut e = edit_of(&v, &id);
    e.title = "Renamed".into();
    assert!(e.autotype.is_none());
    v.update_entry(&id, &e).unwrap();
    assert_eq!(
        v.editable(&id).unwrap().autotype.sequence,
        "{PASSWORD}{ENTER}"
    );
    let _ = fs::remove_dir_all(dir);
}
