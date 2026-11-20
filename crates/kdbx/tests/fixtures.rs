//! Reads, saves and reopens the shared test databases in tests/fixtures.

use std::path::PathBuf;

use kdbx::{Database, Error, Key};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(rel)
}

fn key(pw: bool, keyfile: Option<&str>) -> Key {
    let mut k = Key::new();
    if pw {
        k = k.with_password("mikoshi-test");
    }
    if let Some(f) = keyfile {
        k = k.with_keyfile(&std::fs::read(fixture(f)).unwrap()).unwrap();
    }
    k
}

const CASES: &[(&str, bool, Option<&str>)] = &[
    ("kxc/kdbx31-pw.kdbx", true, None),
    ("kxc/kdbx40-pw.kdbx", true, None),
    ("kxc/kdbx41-pw.kdbx", true, None),
    (
        "kxc/kdbx31-arbitrary.kdbx",
        false,
        Some("keys/arbitrary.dat"),
    ),
    ("kxc/kdbx31-bin32.kdbx", false, Some("keys/binary-32.key")),
    ("kxc/kdbx31-hex64.kdbx", false, Some("keys/hex-64.key")),
    ("kxc/kdbx31-keyxml1.kdbx", false, Some("keys/xml-v1.key")),
    ("kxc/kdbx41-keyxml2.kdbx", false, Some("keys/xml-v2.keyx")),
    ("kxc/kdbx41-pw-keyxml2.kdbx", true, Some("keys/xml-v2.keyx")),
    (
        "kxc/kdbx41-pw-arbitrary.kdbx",
        true,
        Some("keys/arbitrary.dat"),
    ),
];

#[test]
fn opens_and_round_trips_every_fixture() {
    for (file, pw, kf) in CASES {
        let k = key(*pw, *kf);
        let data = std::fs::read(fixture(file)).unwrap();
        let db = Database::open(&data, &k).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(db.entries().len() > 10, "{file}");
        let saved = db.save(&k).unwrap();
        assert_eq!(kdbx::version(&saved).unwrap(), (4, 1));
        let again = Database::open(&saved, &k).unwrap_or_else(|e| panic!("{file} reopen: {e}"));
        assert!(again.document() == db.document(), "{file}: tree changed");
        assert!(
            again.binaries() == db.binaries(),
            "{file}: attachments changed"
        );
    }
}

#[test]
fn wrong_keys_are_told_apart_from_damage() {
    for file in ["kxc/kdbx31-pw.kdbx", "kxc/kdbx41-pw.kdbx"] {
        let data = std::fs::read(fixture(file)).unwrap();
        let wrong = Key::new().with_password("nope");
        assert_eq!(
            Database::open(&data, &wrong).unwrap_err(),
            Error::WrongKey,
            "{file}"
        );
        let cut = &data[..data.len() / 2];
        assert!(
            matches!(
                Database::open(cut, &key(true, None)).unwrap_err(),
                Error::Corrupt(_)
            ),
            "{file}"
        );
    }
}

#[test]
fn fuzzed_input_never_panics() {
    use kdbx::fuzz::{Budget, Corpus, run};
    let k = key(true, None);
    let data = std::fs::read(fixture("kxc/kdbx41-pw.kdbx")).unwrap();
    let db = Database::open(&data, &k).unwrap();
    let data31 = std::fs::read(fixture("kxc/kdbx31-pw.kdbx")).unwrap();
    let xml3 = Database::inner_xml(&data31, &k)
        .unwrap()
        .as_bytes()
        .to_vec();
    let keyfiles = ["keys/xml-v1.key", "keys/xml-v2.keyx", "keys/hex-64.key"]
        .iter()
        .map(|f| std::fs::read(fixture(f)).unwrap())
        .collect();
    let corpus = Corpus::from_databases(&[&db], k, vec![xml3], keyfiles);
    let cases = std::env::var("KDBX_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let out = run(&corpus, 0x6d696b6f736869, Budget::Cases(cases));
    eprintln!("fuzz: {} cases, {} accepted", out.cases, out.accepted);
    assert!(out.failures.is_empty(), "{:#?}", out.failures);
    assert!(
        out.accepted > cases / 10,
        "mutations should often still parse: {out:?}"
    );
}

fn open(file: &str) -> Database {
    let data = std::fs::read(fixture(file)).unwrap();
    Database::open(&data, &key(true, None)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

fn reopen(db: &Database) -> Database {
    let k = key(true, None);
    Database::open(&db.save(&k).unwrap(), &k).unwrap()
}

fn entry_titled<'a>(db: &'a Database, title: &str) -> kdbx::Entry<'a> {
    db.entries()
        .into_iter()
        .find(|e| e.get("Title").as_deref() == Some(title))
        .unwrap_or_else(|| panic!("no entry {title}"))
}

#[test]
fn k1_custom_icons_keep_the_standard_icon_number() {
    let db = reopen(&open("kxc/kdbx41-pw.kdbx"));
    let with_custom: Vec<_> = db
        .entries()
        .into_iter()
        .filter(|e| e.custom_icon().is_some())
        .collect();
    assert!(!with_custom.is_empty(), "the fixture has custom icons");
    for e in with_custom {
        assert!(e.icon_id().is_some(), "IconID kept next to CustomIconUUID");
    }
}

#[test]
fn k2_whitespace_only_values_stay() {
    for file in ["kxc/kdbx31-pw.kdbx", "kxc/kdbx41-pw.kdbx"] {
        let db = reopen(&open(file));
        let values: Vec<String> = db
            .entries()
            .iter()
            .flat_map(|e| {
                e.fields()
                    .map(|f| f.value().into_owned())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(values.iter().any(|v| v == "   "), "{file}");
        assert!(values.iter().any(|v| v == "  padded value  "), "{file}");
    }
}

#[test]
fn k6_k7_unknown_elements_survive_a_save() {
    let db = reopen(&open("foreign/k6-k7-unknown-elements.kdbx"));
    let xml = db.plain_xml();
    for name in [
        "<MikoshiUnknownMeta>",
        "<MikoshiUnknownRoot>",
        "<MikoshiUnknownEntry Flag=\"1\"><Nested>",
        "<MikoshiUnknownAutoType>",
        "<MasterKeyChangeForceOnce>True</MasterKeyChangeForceOnce>",
    ] {
        assert!(xml.contains(name), "{name}");
    }
}

#[test]
fn k8_unknown_element_in_a_group_opens() {
    let db = reopen(&open("foreign/k8-unknown-in-group.kdbx"));
    assert!(db.plain_xml().contains("<MikoshiUnknownGroup>"));
    assert!(db.groups().iter().any(|g| g.name() == "General"));
}

#[test]
fn k9_malformed_timestamp_is_kept_not_fatal() {
    let db = reopen(&open("foreign/k9-bad-timestamp.kdbx"));
    assert!(
        db.plain_xml()
            .contains("<LastAccessTime>AAAA</LastAccessTime>")
    );
    // the views just do not see that one time
    let broken = db
        .entries()
        .into_iter()
        .find(|e| {
            e.element()
                .child("Times")
                .and_then(|t| t.child_text("LastAccessTime"))
                .as_deref()
                == Some("AAAA")
        })
        .unwrap();
    assert_eq!(broken.times().last_access, None);
    assert!(broken.times().creation.is_some());
}

#[test]
fn kdbx31_is_upgraded_on_open_and_reads_like_41() {
    let a = open("kxc/kdbx31-pw.kdbx");
    let b = open("kxc/kdbx41-pw.kdbx");
    assert_eq!(a.opened_version(), (3, 1));
    assert_eq!(a.inner_stream(), kdbx::stream::CHACHA20);
    let meta = a.meta().element();
    assert!(meta.child("HeaderHash").is_none() && meta.child("Binaries").is_none());
    // the same content either way: times in both encodings, attachments
    let full_a = entry_titled(&a, "Full Fields");
    let full_b = entry_titled(&b, "Full Fields");
    assert!(full_a.times().creation.is_some());
    assert_eq!(full_a.times().creation, full_b.times().creation);
    assert_eq!(
        full_a.times().last_modification,
        full_b.times().last_modification
    );
    let atts = |d: &Database| -> Vec<(String, Vec<u8>)> {
        d.entries()
            .iter()
            .flat_map(|e| {
                e.attachments()
                    .map(|x| (x.name.into_owned(), x.data.unwrap().into_owned()))
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    assert_eq!(atts(&a), atts(&b));
    assert!(atts(&a).len() >= 6);
    // time text in the 3.1 tree is Base64 now, as KDBX 4 readers need
    let t = full_a
        .element()
        .child("Times")
        .unwrap()
        .child_text("CreationTime")
        .unwrap();
    assert_eq!(kdbx::Timestamp::parse_base64(&t), full_a.times().creation);
}

#[test]
fn views_read_groups_entries_and_settings() {
    let db = open("kxc/kdbx41-pw.kdbx");
    assert!(!db.meta().database_name().is_empty());
    assert!(db.recycle_bin().is_some());
    assert!(!db.meta().custom_icons().is_empty());
    let custom = entry_titled(&db, "AutoType Custom");
    let at = custom.autotype().unwrap();
    assert!(at.enabled && at.associations.len() == 3);
    assert!(custom.field("Password").unwrap().protected());
    assert!(!custom.field("UserName").unwrap().protected());
    let full = entry_titled(&db, "Full Fields");
    assert!(db.parent_of_entry(full.uuid().unwrap()).is_some());
    assert!(db.entries().iter().any(|e| e.history().count() == 3));
    let s = db.settings();
    assert_eq!(s.cipher, kdbx::Cipher::Aes256);
    assert!(s.compressed);
    assert!(matches!(s.kdf, kdbx::Kdf::Aes { .. }));
}

#[test]
fn edits_change_only_their_nodes() {
    let mut db = open("kxc/kdbx41-pw.kdbx");
    let before = db.document().clone();
    let id = entry_titled(&db, "Full Fields").uuid().unwrap();
    db.entry_mut(id)
        .unwrap()
        .set_field("UserName", "someone-else", false);
    let after = reopen(&db);
    let e = after.entry(id).unwrap();
    assert_eq!(e.get("UserName").as_deref(), Some("someone-else"));
    // put it back: the whole tree is as before
    let mut again = after;
    again.entry_mut(id).unwrap().set_field(
        "UserName",
        &entry_titled(&open("kxc/kdbx41-pw.kdbx"), "Full Fields")
            .get("UserName")
            .unwrap(),
        false,
    );
    assert!(*again.document() == before);
}

fn set_meta(db: &mut Database, name: &str, value: &str) {
    let meta = db.meta_element_mut().unwrap();
    match meta.child_mut(name) {
        Some(e) => e.set_text(value),
        None => meta.insert_element(kdbx::xml::Element::with_text(name, value), &[], &[]),
    }
}

fn history_titles(db: &Database, id: kdbx::Uuid) -> Vec<String> {
    db.entry(id)
        .unwrap()
        .history()
        .map(|h| h.get("Title").unwrap_or_default().into_owned())
        .collect()
}

#[test]
fn history_is_trimmed_to_the_database_limits() {
    let mut db = open("kxc/kdbx41-pw.kdbx");
    set_meta(&mut db, "HistoryMaxItems", "3");
    set_meta(&mut db, "HistoryMaxSize", "-1");
    let id = entry_titled(&db, "Full Fields").uuid().unwrap();
    let before = history_titles(&db, id).len();
    for i in 0..5 {
        let mut e = db.entry_mut(id).unwrap();
        e.push_history();
        e.set_field("Title", &format!("v{i}"), false);
        db.maintain_history(id);
    }
    // oldest go first; the newest three remain
    let h = history_titles(&db, id);
    assert_eq!(h.len(), 3, "had {before} before");
    assert_eq!(h, ["v1", "v2", "v3"]);
    assert_eq!(history_titles(&reopen(&db), id), h);

    // no limit on the count, a small one on the size
    set_meta(&mut db, "HistoryMaxItems", "-1");
    set_meta(&mut db, "HistoryMaxSize", "0");
    db.maintain_history(id);
    assert!(history_titles(&db, id).is_empty());

    // HistoryMaxItems 0 keeps no history at all
    set_meta(&mut db, "HistoryMaxSize", "-1");
    set_meta(&mut db, "HistoryMaxItems", "0");
    db.entry_mut(id).unwrap().push_history();
    assert_eq!(db.maintain_history(id), 1);
}

#[test]
fn restoring_history_keeps_the_current_version() {
    let mut db = open("kxc/kdbx41-pw.kdbx");
    set_meta(&mut db, "HistoryMaxItems", "10");
    let id = entry_titled(&db, "Full Fields").uuid().unwrap();
    let original_pw = db.entry(id).unwrap().get("Password").unwrap().into_owned();
    let n = history_titles(&db, id).len();
    {
        let mut e = db.entry_mut(id).unwrap();
        e.push_history();
        e.set_field("Title", "Changed", false);
        e.set_field("Password", "new-secret", true);
        e.touch();
    }
    assert!(db.restore_history(id, n));
    let e = db.entry(id).unwrap();
    assert_eq!(e.get("Title").as_deref(), Some("Full Fields"));
    assert_eq!(e.get("Password").as_deref(), Some(original_pw.as_str()));
    assert!(e.field("Password").unwrap().protected());
    // the version that was current is now the newest history item
    let h: Vec<_> = e.history().collect();
    assert_eq!(h.len(), n + 2);
    assert_eq!(h[n + 1].get("Title").as_deref(), Some("Changed"));
    assert_eq!(h[n + 1].get("Password").as_deref(), Some("new-secret"));
    assert!(h.iter().all(|v| v.element().child("History").is_none()));
    assert_eq!(e.uuid(), Some(id));

    let again = reopen(&db);
    assert!(again.document() == db.document());
    assert!(!db.restore_history(id, 999));
}

#[test]
fn renames_tags_and_expiry_are_saved() {
    const STANDARD: [&str; 5] = ["Title", "UserName", "Password", "URL", "Notes"];
    let mut db = open("kxc/kdbx41-pw.kdbx");
    let id = db
        .entries()
        .into_iter()
        .find(|e| e.fields().any(|f| !STANDARD.contains(&f.key().as_ref())))
        .expect("an entry with a custom field")
        .uuid()
        .unwrap();
    let keys: Vec<String> = db
        .entry(id)
        .unwrap()
        .fields()
        .map(|f| f.key().into_owned())
        .collect();
    let custom = keys
        .iter()
        .find(|k| !STANDARD.contains(&k.as_str()))
        .expect("a custom field")
        .clone();
    let when = kdbx::Timestamp::from_unix(2_000_000_000);
    {
        let mut e = db.entry_mut(id).unwrap();
        assert!(e.rename_field(&custom, "Renamed"));
        assert!(!e.rename_field("Renamed", "Title"));
        assert!(!e.rename_field("no such field", "x"));
        e.set_tags(&["one".into(), " two ".into(), "".into()]);
        e.set_expiry(Some(when));
    }
    let again = reopen(&db);
    let e = again.entry(id).unwrap();
    let renamed: Vec<String> = e.fields().map(|f| f.key().into_owned()).collect();
    let pos = keys.iter().position(|k| *k == custom).unwrap();
    assert_eq!(renamed[pos], "Renamed", "renamed in place");
    assert_eq!(e.tags(), ["one", "two"]);
    assert_eq!(e.times().expires_at(), Some(when));

    let mut db = again;
    db.entry_mut(id).unwrap().set_expiry(None);
    db.entry_mut(id).unwrap().set_tags(&[]);
    let e = db.entry(id).unwrap();
    assert_eq!(e.times().expires_at(), None);
    assert_eq!(e.times().expiry, Some(when), "the old time is kept");
    assert!(e.tags().is_empty());
}

#[test]
fn a_new_entry_with_an_empty_protected_password_round_trips() {
    let mut db = open("kxc/kdbx41-pw.kdbx");
    let root = db.root_group().unwrap().uuid().unwrap();
    let id = db.add_entry(root).unwrap();
    db.entry_mut(id).unwrap().set_field("Title", "New", false);
    let again = reopen(&db);
    assert!(again.document() == db.document());
    assert_eq!(
        again.entry(id).unwrap().get("Password").as_deref(),
        Some("")
    );
}

// ---------------------------------------------------------------- M8

fn group_named(db: &Database, name: &str) -> kdbx::Uuid {
    db.groups()
        .into_iter()
        .find(|g| g.name() == name)
        .unwrap_or_else(|| panic!("no group {name}"))
        .uuid()
        .unwrap()
}

fn deleted_ids(db: &Database) -> Vec<kdbx::Uuid> {
    db.document()
        .child("Root")
        .and_then(|r| r.child("DeletedObjects"))
        .map(|d| {
            d.children_named("DeletedObject")
                .filter_map(|o| kdbx::Uuid::from_base64(&o.child_text("UUID")?))
                .collect()
        })
        .unwrap_or_default()
}

/// Every attachment of every entry (history included) with its bytes.
fn all_attachments(db: &Database) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for e in db.entries() {
        for v in std::iter::once(e).chain(e.history().collect::<Vec<_>>()) {
            for a in v.attachments() {
                out.push((
                    a.name.into_owned(),
                    a.data.map(|d| d.into_owned()).unwrap_or_default(),
                ));
            }
        }
    }
    out.sort();
    out
}

fn fast_settings(cipher: kdbx::Cipher) -> kdbx::Settings {
    kdbx::Settings {
        cipher,
        kdf: kdbx::Kdf::Argon2 {
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
fn groups_rename_and_move_but_not_into_themselves() {
    let mut db = open("kxc/kdbx41-pw.kdbx");
    let nested = group_named(&db, "Nested");
    let level2 = group_named(&db, "Level 2");
    let general = group_named(&db, "General");
    assert!(!db.move_group(nested, level2), "into its own subgroup");
    assert!(!db.move_group(nested, nested), "into itself");
    let root = db.root_group().unwrap().uuid().unwrap();
    assert!(!db.move_group(root, general), "the root group stays");

    assert!(db.rename_group(nested, "Renamed"));
    assert!(db.move_group(nested, general));
    assert_eq!(db.parent_of_group(nested), Some(general));
    assert_eq!(
        db.parent_of_group(level2),
        Some(nested),
        "children move along"
    );
    assert_eq!(db.previous_parent(nested), Some(root));

    let again = reopen(&db);
    assert!(again.document() == db.document());
    assert_eq!(again.group(nested).unwrap().name(), "Renamed");
}

#[test]
fn deleting_for_good_records_deleted_objects() {
    let mut db = open("kxc/kdbx41-pw.kdbx");
    let entry = entry_titled(&db, "Full Fields").uuid().unwrap();
    assert!(db.delete_entry(entry));
    assert!(db.entry(entry).is_none());
    assert!(!db.delete_entry(entry), "already gone");
    assert!(deleted_ids(&db).contains(&entry));

    let nested = group_named(&db, "Nested");
    let level2 = group_named(&db, "Level 2");
    let mut inside = Vec::new();
    for g in db.groups() {
        let gid = g.uuid().unwrap();
        let mut cur = Some(gid);
        let mut below = false;
        while let Some(c) = cur {
            if c == nested {
                below = true;
                break;
            }
            cur = db.parent_of_group(c);
        }
        if below {
            inside.push(gid);
            inside.extend(g.entries().filter_map(|e| e.uuid()));
        }
    }
    assert!(inside.contains(&level2));
    assert!(db.delete_group(nested));
    assert!(db.group(level2).is_none());
    let deleted = deleted_ids(&db);
    for id in &inside {
        assert!(deleted.contains(id));
    }
    let root = db.root_group().unwrap().uuid().unwrap();
    assert!(!db.delete_group(root));

    let again = reopen(&db);
    assert!(again.document() == db.document());
    assert!(again.binaries() == db.binaries());
}

#[test]
fn attachments_leave_the_pool_when_nothing_uses_them() {
    let mut db = open("kxc/kdbx41-pw.kdbx");
    let before = all_attachments(&db);
    let pool = db.binaries().len();
    let (id, name) = db
        .entries()
        .into_iter()
        .find_map(|e| {
            let a = e.attachments().next()?;
            Some((e.uuid()?, a.name.into_owned()))
        })
        .expect("an entry with an attachment");
    assert!(db.remove_attachment(id, &name));
    assert!(!db.remove_attachment(id, &name), "already gone");
    assert!(db.entry(id).unwrap().attachments().all(|a| a.name != name));

    // every other attachment still reads the same bytes
    let after = all_attachments(&db);
    let mut expected = before.clone();
    let i = expected.iter().position(|(n, _)| *n == name).unwrap();
    expected.remove(i);
    assert_eq!(after, expected);
    assert!(db.binaries().len() <= pool);
    let again = reopen(&db);
    assert_eq!(all_attachments(&again), after);
    assert!(again.binaries() == db.binaries());
}

#[test]
fn removing_every_attachment_empties_the_pool() {
    let mut db = open("kxc/kdbx41-pw.kdbx");
    let mut ids = Vec::new();
    for e in db.entries() {
        for a in e.attachments() {
            ids.push((e.uuid().unwrap(), a.name.into_owned()));
        }
    }
    for (id, name) in &ids {
        assert!(db.remove_attachment(*id, name));
    }
    // older versions may keep theirs; the pool holds only what they use
    let still_used = all_attachments(&db).len();
    if still_used == 0 {
        assert!(db.binaries().is_empty());
    }
    let again = reopen(&db);
    assert!(again.binaries() == db.binaries());
}

#[test]
fn picking_a_standard_icon_drops_the_custom_one() {
    let mut db = open("kxc/kdbx41-pw.kdbx");
    let id = entry_titled(&db, "Custom Icon Entry").uuid().unwrap();
    assert!(db.entry(id).unwrap().custom_icon().is_some());
    db.entry_mut(id).unwrap().set_icon(12);
    let again = reopen(&db);
    let e = again.entry(id).unwrap();
    assert_eq!(e.icon_id(), Some(12));
    assert_eq!(e.custom_icon(), None);
}

#[test]
fn a_recycle_bin_is_made_when_missing() {
    let mut db = Database::create("New <&> DB", &fast_settings(kdbx::Cipher::Aes256)).unwrap();
    assert_eq!(db.meta().database_name(), "New <&> DB");
    assert!(db.recycle_bin().is_none());
    let bin = db.ensure_recycle_bin("Recycle Bin").unwrap();
    assert_eq!(db.ensure_recycle_bin("other name"), Some(bin), "made once");
    assert_eq!(db.meta().recycle_bin(), Some(bin));
    let g = db.group(bin).unwrap();
    assert_eq!(g.enable_searching(), Some(false));
    assert_eq!(g.enable_autotype(), Some(false));

    // deleting the bin itself clears the setting
    assert!(db.delete_group(bin));
    assert_eq!(db.meta().recycle_bin(), None);
    let again = reopen(&db);
    assert!(again.document() == db.document());
}

#[test]
fn new_databases_open_with_either_cipher() {
    for cipher in [kdbx::Cipher::Aes256, kdbx::Cipher::ChaCha20] {
        let settings = fast_settings(cipher);
        let mut db = Database::create("Mine", &settings).unwrap();
        let root = db.root_group().unwrap().uuid().unwrap();
        assert_eq!(db.root_group().unwrap().name(), "Mine");
        let id = db.add_entry(root).unwrap();
        db.entry_mut(id).unwrap().set_field("Title", "First", false);
        db.entry_mut(id)
            .unwrap()
            .set_field("Password", "s3cret", true);

        let k = key(true, None);
        let data = db.save(&k).unwrap();
        assert_eq!(kdbx::version(&data).unwrap(), (4, 1));
        let again = Database::open(&data, &k).unwrap();
        assert_eq!(again.settings(), settings);
        let e = again.entry(id).unwrap();
        assert_eq!(e.get("Password").as_deref(), Some("s3cret"));
        assert!(e.field("Password").unwrap().protected());
        assert!(again.document() == db.document());
    }
}

#[test]
fn calibration_scales_iterations_to_the_target() {
    let kdf = kdbx::calibrate_argon2id(1 << 20, 1, std::time::Duration::from_millis(50)).unwrap();
    let kdbx::Kdf::Argon2 { id, iterations, .. } = kdf else {
        panic!("not argon2")
    };
    assert!(id);
    assert!((2..=10_000).contains(&iterations));
    let took = kdbx::time_kdf(&kdf).unwrap();
    // generous: the machine may be busy
    assert!(took < std::time::Duration::from_millis(500), "{took:?}");
}
