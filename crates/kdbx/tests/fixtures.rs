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
