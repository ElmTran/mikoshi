//! Fuzzing support, hidden from the docs: mutates whole files and decrypted
//! payloads, feeds them to the reader, and checks that nothing panics and
//! that whatever is accepted saves and reopens to the same tree.
//!
//! Whole-file mutations mostly stop at the checksums, so the decrypted
//! layers (inner header, XML, the 3.1 conversion, gzip) are also fed
//! directly, without the encryption in front.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::crypto::{Cipher, STREAM_SALSA20};
use crate::db::{self, Database};
use crate::header::{COMPRESSION_GZIP, InnerHeader, Kdf, OuterHeader, VariantDict};
use crate::key::Key;
use crate::xml;

/// xorshift64*: deterministic and good enough to pick mutations.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

const INTERESTING: &[&[u8]] = &[
    b"\0",
    b"\xff",
    b"<",
    b">",
    b"&",
    b"\"",
    b"/",
    b"=",
    b"</Entry>",
    b"<Group>",
    b"<Value Protected=\"True\">",
    b"&#0;",
    b"&#xD;",
    b"]]>",
    b"<!--",
    b"AAAA",
    b"\xff\xff\xff\xff",
    b"\x00\x00\x00\x80",
    b"\x7f\xff\xff\xff",
];

/// One random change: bit flips, overwrites, cuts, insertions, duplication.
pub fn mutate(rng: &mut Rng, input: &[u8]) -> Vec<u8> {
    let mut d = input.to_vec();
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        let len = d.len();
        match rng.below(8) {
            0 if len > 0 => {
                for _ in 0..1 + rng.below(8) {
                    let i = rng.below(len);
                    d[i] ^= 1 << rng.below(8);
                }
            }
            1 if len > 0 => {
                let i = rng.below(len);
                d[i] = rng.next_u64() as u8;
            }
            2 => d.truncate(rng.below(len + 1)),
            3 if len > 0 => {
                let a = rng.below(len);
                let b = (a + 1 + rng.below(64)).min(len);
                d.drain(a..b);
            }
            4 => {
                let at = rng.below(len + 1);
                let n = 1 + rng.below(16);
                let bytes: Vec<u8> = (0..n).map(|_| rng.next_u64() as u8).collect();
                d.splice(at..at, bytes);
            }
            5 if len > 0 => {
                let a = rng.below(len);
                let b = (a + 1 + rng.below(256)).min(len);
                let chunk = d[a..b].to_vec();
                let at = rng.below(len + 1);
                d.splice(at..at, chunk);
            }
            6 => {
                let at = rng.below(len + 1);
                let x = INTERESTING[rng.below(INTERESTING.len())];
                let end = (at + rng.below(x.len() + 1)).min(len);
                d.splice(at..end, x.iter().copied());
            }
            _ if len > 4 => {
                // little-endian length fields: make one huge or zero
                let i = rng.below(len - 4);
                let v: u32 = match rng.below(3) {
                    0 => 0,
                    1 => u32::MAX,
                    _ => rng.next_u64() as u32,
                };
                d[i..i + 4].copy_from_slice(&v.to_le_bytes());
            }
            _ => {}
        }
    }
    d
}

/// Inputs to mutate.
pub struct Corpus {
    /// whole files that use a cheap KDF, and the key that opens them
    pub files: Vec<Vec<u8>>,
    pub key: Key,
    /// decrypted KDBX 4 payloads (inner header + XML)
    pub payloads4: Vec<Vec<u8>>,
    /// decrypted KDBX 3.1 XML
    pub xml3: Vec<Vec<u8>>,
    /// key files
    pub keyfiles: Vec<Vec<u8>>,
}

impl Corpus {
    /// From open databases: each is saved with a one-round AES-KDF under
    /// `key`. `xml3` is KDBX 3.1 XML from [`Database::inner_xml`].
    pub fn from_databases(
        dbs: &[&Database],
        key: Key,
        xml3: Vec<Vec<u8>>,
        keyfiles: Vec<Vec<u8>>,
    ) -> Corpus {
        let mut files = Vec::new();
        let mut payloads4 = Vec::new();
        for db in dbs {
            let mut cheap = clone_db(db);
            let mut s = cheap.settings();
            s.kdf = Kdf::Aes { rounds: 1 };
            cheap.set_settings(&s);
            if let Ok(f) = cheap.save(&key) {
                files.push(f);
            }
            payloads4.push(payload4_of(db).to_vec());
        }
        Corpus {
            files,
            key,
            payloads4,
            xml3,
            keyfiles,
        }
    }
}

fn clone_db(db: &Database) -> Database {
    Database {
        header: db.header.clone(),
        stream_id: db.stream_id,
        binaries: db.binaries.clone(),
        root: db.root.clone(),
        opened_version: db.opened_version,
        index: Default::default(),
    }
    .reindexed()
}

impl Database {
    fn reindexed(mut self) -> Database {
        self.reindex();
        self
    }
}

/// The decrypted KDBX 4 payload a save would encrypt.
pub fn payload4_of(db: &Database) -> Zeroizing<Vec<u8>> {
    let inner = InnerHeader {
        stream_id: db.stream_id,
        stream_key: Zeroizing::new(vec![7; 64]),
        binaries: db.binaries.clone(),
        len: 0,
    };
    let mut stream = crate::crypto::InnerStream::new(inner.stream_id, &inner.stream_key)
        .unwrap_or(crate::crypto::InnerStream::None);
    let mut out = Zeroizing::new(Vec::new());
    inner.write(&mut out);
    out.extend_from_slice(&xml::write(&db.root, &mut stream));
    out
}

fn cheap_header(major: u16) -> OuterHeader {
    OuterHeader {
        major,
        minor: 1,
        cipher: Cipher::Aes256,
        compression: COMPRESSION_GZIP,
        master_seed: vec![0; 32],
        iv: vec![0; 16],
        kdf: Kdf::Aes { rounds: 1 }.to_params(&VariantDict::new()),
        public_custom_data: None,
        stream_key: (major < 4).then(|| Zeroizing::new(vec![1; 32])),
        stream_start: (major < 4).then(|| vec![0; 32]),
        stream_id: (major < 4).then_some(STREAM_SALSA20),
        len: 0,
    }
}

#[derive(Default, Debug)]
pub struct Outcome {
    pub cases: u64,
    /// mutated inputs the reader accepted
    pub accepted: u64,
    /// panics and round-trip mismatches, with how to reproduce them
    pub failures: Vec<String>,
}

pub enum Budget {
    Cases(u64),
    Time(Duration),
}

/// Reads everything the views offer, then saves and reopens. `Err` when the
/// reopened tree differs.
fn exercise(db: &Database, key: &Key) -> Result<(), &'static str> {
    let m = db.meta();
    let _ = (m.database_name(), m.recycle_bin(), m.custom_icons().len());
    let _ = db.recycle_bin().map(|g| g.uuid());
    for g in db.groups() {
        let _ = (
            g.uuid(),
            g.name(),
            g.notes(),
            g.icon_id(),
            g.custom_icon(),
            g.tags(),
        );
        let _ = (
            g.times(),
            g.is_expanded(),
            g.enable_searching(),
            g.enable_autotype(),
        );
        let _ = g.default_autotype_sequence();
    }
    for e in db.entries() {
        let check = |e: &crate::Entry<'_>| {
            let _ = (
                e.uuid(),
                e.icon_id(),
                e.custom_icon(),
                e.tags(),
                e.times().expired(),
            );
            for f in e.fields() {
                let _ = (f.key(), f.value(), f.protected());
            }
            for a in e.attachments() {
                let _ = (a.name, a.data.map(|d| d.len()));
            }
            let _ = e.autotype();
        };
        check(&e);
        e.history().for_each(|h| check(&h));
        if let Some(id) = e.uuid() {
            let _ = db.parent_of_entry(id).map(|g| g.name());
        }
    }
    let _ = (db.settings(), db.plain_xml().len());
    let saved = db.save(key).map_err(|_| "save failed")?;
    let again = Database::open(&saved, key).map_err(|_| "cannot reopen the saved file")?;
    if again.root != db.root {
        return Err("the tree changed in a save and reopen");
    }
    if again.binaries != db.binaries {
        return Err("attachments changed in a save and reopen");
    }
    Ok(())
}

/// Runs mutated inputs until the budget is spent.
pub fn run(corpus: &Corpus, seed: u64, budget: Budget) -> Outcome {
    let mut rng = Rng::new(seed);
    let mut out = Outcome::default();
    let started = Instant::now();
    let cheap_key = Key::new().with_password("fuzz");
    // keep the expected panics' messages out of the console while running
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    loop {
        match budget {
            Budget::Cases(n) if out.cases >= n => break,
            Budget::Time(t) if started.elapsed() >= t => break,
            _ => {}
        }
        let case = out.cases;
        out.cases += 1;
        let target = case % 5;
        let mut accepted = false;
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), &'static str> {
            match target {
                0 if !corpus.files.is_empty() => {
                    let pick = rng.below(corpus.files.len());
                    let input = mutate(&mut rng, &corpus.files[pick]);
                    if let Ok(db) = Database::open(&input, &corpus.key) {
                        accepted = true;
                        exercise(&db, &cheap_key)?;
                    }
                }
                1 if !corpus.payloads4.is_empty() => {
                    let pick = rng.below(corpus.payloads4.len());
                    let input = mutate(&mut rng, &corpus.payloads4[pick]);
                    if let Ok(db) = db::finish4(cheap_header(4), &input) {
                        accepted = true;
                        exercise(&db, &cheap_key)?;
                    }
                }
                2 if !corpus.xml3.is_empty() => {
                    let pick = rng.below(corpus.xml3.len());
                    let input = mutate(&mut rng, &corpus.xml3[pick]);
                    if let Ok(db) = db::finish3(cheap_header(3), Zeroizing::new(input), [0; 32]) {
                        accepted = true;
                        exercise(&db, &cheap_key)?;
                    }
                }
                3 if !corpus.payloads4.is_empty() => {
                    // the gzip layer
                    use std::io::Write as _;
                    let src = &corpus.payloads4[rng.below(corpus.payloads4.len())];
                    let mut gz =
                        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
                    let _ = gz.write_all(src);
                    let packed = gz.finish().unwrap_or_default();
                    let input = mutate(&mut rng, &packed);
                    if let Ok(plain) = db::gunzip(&input)
                        && let Ok(db) = db::finish4(cheap_header(4), &plain)
                    {
                        accepted = true;
                        exercise(&db, &cheap_key)?;
                    }
                }
                _ => {
                    let src: &[u8] = corpus
                        .keyfiles
                        .get(rng.below(corpus.keyfiles.len().max(1)))
                        .map_or(&[], Vec::as_slice);
                    let input = mutate(&mut rng, src);
                    if Key::new().with_keyfile(&input).is_ok() {
                        accepted = true;
                    }
                    let _ = VariantDict::parse(&input);
                }
            }
            Ok(())
        }));
        if accepted {
            out.accepted += 1;
        }
        match result {
            Ok(Ok(())) => {}
            Ok(Err(why)) => out
                .failures
                .push(format!("seed {seed}, case {case} (target {target}): {why}")),
            Err(p) => {
                let msg = p
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                out.failures.push(format!(
                    "seed {seed}, case {case} (target {target}): PANIC {msg}"
                ));
            }
        }
        if out.failures.len() >= 20 {
            break;
        }
    }
    std::panic::set_hook(previous);
    out
}
