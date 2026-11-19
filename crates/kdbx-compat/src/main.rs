//! kdbx-compat: checks that the kdbx crate reads KeePass files exactly like
//! KeePassXC does, and that saving them loses nothing.
//!
//!   kdbx-compat [fixtures]          run every check on tests/fixtures, write the report
//!   kdbx-compat check <file> [--keyfile <path>] [--no-password | --password-stdin]
//!                                   round-trip one file in memory and list what would
//!                                   change; prints paths only, never values, and never
//!                                   writes anything
//!   kdbx-compat timing              time opening files with typical KDF settings
//!   kdbx-compat fuzz [seconds] [seed]
//!                                   feed mutated files and payloads to the reader

mod canon;
mod header;
mod kxc;
mod model;
mod writer;

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use kdbx::{Cipher, Database, Kdf};
use serde::Deserialize;

use canon::{Canon, Change, Diff, Source};
use kxc::{Key, Kxc};

// ---------------------------------------------------------------- shared

fn workspace_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or(here)
}

fn out_dir() -> PathBuf {
    workspace_root().join("target").join("kdbx-compat")
}

#[derive(Debug)]
enum OpenError {
    Failed(kdbx::Error),
    Key(String),
    Panicked(String),
}

fn label(e: &kdbx::Error) -> String {
    match e {
        kdbx::Error::WrongKey => "wrong key".into(),
        kdbx::Error::Corrupt(w) => format!("damaged ({w})"),
        kdbx::Error::Unsupported(w) => format!("unsupported ({w})"),
        kdbx::Error::NotKeePass => "not a KeePass file".into(),
        kdbx::Error::KeePass1 => "KeePass 1 file".into(),
        other => other.to_string(),
    }
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Failed(e) => f.write_str(&label(e)),
            OpenError::Key(msg) => write!(f, "key file: {msg}"),
            OpenError::Panicked(msg) => write!(f, "PANIC: {msg}"),
        }
    }
}

fn panic_message(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

/// Opens a database; a panic is reported as an error (and is always a bug).
fn open(bytes: &[u8], key: &Key) -> Result<Database, OpenError> {
    let k = key.to_kdbx().map_err(|e| OpenError::Key(e.to_string()))?;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(|| Database::open(bytes, &k)));
    std::panic::set_hook(previous);
    match result {
        Ok(Ok(db)) => Ok(db),
        Ok(Err(e)) => Err(OpenError::Failed(e)),
        Err(p) => Err(OpenError::Panicked(panic_message(p))),
    }
}

fn raw_xml(bytes: &[u8], key: &Key) -> Result<String, String> {
    let k = key.to_kdbx().map_err(|e| e.to_string())?;
    Database::inner_xml(bytes, &k)
        .map(|x| x.to_string())
        .map_err(|e| label(&e))
}

fn save(db: &Database, key: &Key) -> Result<Vec<u8>, String> {
    let k = key.to_kdbx().map_err(|e| e.to_string())?;
    match catch_unwind(AssertUnwindSafe(|| db.save(&k))) {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => Err(label(&e)),
        Err(p) => Err(format!("PANIC: {}", panic_message(p))),
    }
}

/// Our reading of the file, in the form keepassxc-cli exports.
fn our_view(db: &Database) -> Result<Canon, String> {
    canon::parse(&db.plain_xml(), Source::Export)
}

fn ms(d: Duration) -> String {
    format!("{} ms", d.as_millis())
}

fn kdf_name(k: &header::Kdf) -> &'static str {
    match k {
        header::Kdf::Aes { .. } => "AES-KDF",
        header::Kdf::Argon2 { variant, .. } => variant,
        header::Kdf::Unknown(_) => "unknown",
    }
}

// ---------------------------------------------------------------- M0 issues

/// Problems M0 found in keepass-rs. The kdbx crate (M6) must not have any of
/// them; each has a check below, and hitting one is a failure.
pub const M0_ISSUES: &[(&str, &str, &str)] = &[
    (
        "K1",
        "An entry or group with a custom icon lost its standard icon number (IconID)",
        "round trip of every fixture (tree compared node by node)",
    ),
    (
        "K2",
        "A field whose value is only whitespace was read as empty (and saved empty)",
        "read vs KeePassXC, round trip",
    ),
    (
        "K3",
        "TOTP: an otpauth URI without `digits` gave 8 digits",
        "TOTP codes vs KeePassXC (Mikoshi's own TOTP since M3)",
    ),
    (
        "K4",
        "TOTP: a lower-case Base32 secret could not be parsed",
        "TOTP codes vs KeePassXC (Mikoshi's own TOTP since M3)",
    ),
    (
        "K5",
        "KDBX 3.1: a wrong key and a damaged file could not be told apart",
        "wrong key on every fixture; damaged files",
    ),
    (
        "K6",
        "Unknown XML elements were dropped on save",
        "elements KeePassXC does not keep",
    ),
    (
        "K7",
        "Meta/MasterKeyChangeForceOnce was dropped on save",
        "elements KeePassXC does not keep",
    ),
    (
        "K8",
        "An unknown element directly inside a Group made the file fail to open",
        "elements KeePassXC does not keep",
    ),
    (
        "K9",
        "A malformed timestamp caused a panic instead of an error",
        "elements KeePassXC does not keep; fuzzing",
    ),
];

// ---------------------------------------------------------------- diff classification

enum Verdict {
    Benign(&'static str),
    Real,
}

/// `before` is the document the diff started from, for context.
fn classify_diff(d: &Diff, _before: &Canon) -> Verdict {
    let p = d.path.as_str();
    match &d.change {
        Change::Added(_) if p == "Meta/SettingsChanged" => {
            Verdict::Benign("KeePassXC shows a SettingsChanged time once the file is KDBX 4")
        }
        _ => Verdict::Real,
    }
}

struct Sorted<'a> {
    real: Vec<&'a Diff>,
    benign: Vec<(&'a Diff, &'static str)>,
}

fn sort_diffs<'a>(diffs: &'a [Diff], before: &Canon) -> Sorted<'a> {
    let mut s = Sorted {
        real: Vec::new(),
        benign: Vec::new(),
    };
    for d in diffs {
        match classify_diff(d, before) {
            Verdict::Benign(why) => s.benign.push((d, why)),
            Verdict::Real => s.real.push(d),
        }
    }
    s
}

/// Reports a sorted diff into a section. `prefix` says which comparison it is.
fn report_diffs(
    sec: &mut Section,
    sorted: &Sorted<'_>,
    labels: &[&HashMap<String, String>],
    prefix: &str,
    show_values: bool,
) {
    for d in &sorted.real {
        sec.fail(format!("{prefix} {}", describe(d, labels, show_values)));
    }
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, why) in &sorted.benign {
        *kinds.entry(why).or_default() += 1;
    }
    for (why, n) in kinds {
        sec.note(format!("{prefix} harmless ×{n}: {why}"));
    }
}

fn describe(d: &Diff, labels: &[&HashMap<String, String>], show_values: bool) -> String {
    let path = canon::display_path(&d.path, labels);
    let short = |v: &str| {
        let v = v.replace('\n', "\\n");
        if v.chars().count() > 60 {
            format!("{}…", v.chars().take(60).collect::<String>())
        } else {
            v
        }
    };
    match (&d.change, show_values) {
        (Change::Missing(v), true) => format!("- LOST     {path} = \"{}\"", short(v)),
        (Change::Added(v), true) => format!("- ADDED    {path} = \"{}\"", short(v)),
        (Change::Changed(a, b), true) => {
            format!("- CHANGED  {path}: \"{}\" -> \"{}\"", short(a), short(b))
        }
        (Change::Missing(_), false) => format!("- LOST     {path}"),
        (Change::Added(_), false) => format!("- ADDED    {path}"),
        (Change::Changed(_, _), false) => format!("- CHANGED  {path}"),
    }
}

/// The saved file reopened must hold exactly the same tree and attachments.
fn same_tree(sec: &mut Section, before: &Database, after: &Database, what: &str) {
    let diffs = model::tree_diff(before.document(), after.document(), 20);
    if diffs.is_empty() && before.binaries() == after.binaries() {
        sec.note(format!(
            "{what}: XML tree identical node by node (whitespace included), {} attachments identical",
            before.binaries().len()
        ));
        return;
    }
    for d in diffs {
        sec.fail(format!("{what}: tree differs at {d}"));
    }
    if before.binaries() != after.binaries() {
        sec.fail(format!("{what}: attachment pool differs"));
    }
}

// ---------------------------------------------------------------- fixtures mode

#[derive(Deserialize)]
struct Manifest {
    password: String,
    fixture: Vec<FixtureSpec>,
}

#[derive(Deserialize, Clone)]
struct FixtureSpec {
    file: String,
    made_by: String,
    password: bool,
    keyfile: Option<String>,
    format: String,
    cipher: String,
    kdf: String,
    #[serde(default)]
    optional: bool,
}

#[derive(Default)]
struct Section {
    title: String,
    ok: bool,
    lines: Vec<String>,
}

impl Section {
    fn new(title: impl Into<String>) -> Self {
        Section {
            title: title.into(),
            ok: true,
            lines: Vec::new(),
        }
    }
    fn fail(&mut self, line: impl Into<String>) {
        self.ok = false;
        self.lines.push(line.into());
    }
    fn note(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }
    fn check(&mut self, ok: bool, line: impl Into<String>) {
        if ok { self.note(line) } else { self.fail(line) }
    }
    fn status(&self) -> &'static str {
        if self.ok { "ok" } else { "PROBLEMS" }
    }
}

struct FixtureReport {
    file: String,
    made_by: String,
    summary: String,
    status: &'static str,
    sections: Vec<Section>,
}

struct Ctx {
    fixtures: PathBuf,
    manifest: Manifest,
    kxc: Option<Kxc>,
}

impl Ctx {
    fn key(&self, spec: &FixtureSpec) -> Key {
        Key {
            password: spec.password.then(|| self.manifest.password.clone()),
            keyfile: spec.keyfile.as_ref().map(|k| self.fixtures.join(k)),
        }
    }

    fn spec(&self, file: &str) -> Option<FixtureSpec> {
        self.manifest
            .fixture
            .iter()
            .find(|f| f.file == file)
            .cloned()
    }
}

fn load_ctx() -> Result<Ctx, String> {
    let fixtures = workspace_root().join("tests").join("fixtures");
    let manifest_text = std::fs::read_to_string(fixtures.join("fixtures.toml"))
        .map_err(|e| format!("cannot read tests/fixtures/fixtures.toml: {e}"))?;
    let manifest: Manifest =
        toml::from_str(&manifest_text).map_err(|e| format!("bad fixtures.toml: {e}"))?;
    Ok(Ctx {
        fixtures,
        manifest,
        kxc: Kxc::find(),
    })
}

fn run_fixtures() -> ExitCode {
    let ctx = match load_ctx() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let _ = std::fs::create_dir_all(out_dir().join("saved"));
    let _ = std::fs::create_dir_all(out_dir().join("variants"));

    let started = Instant::now();
    let mut reports = Vec::new();
    for spec in ctx.manifest.fixture.clone() {
        eprint!("{:44} ", spec.file);
        let r = check_fixture(&ctx, &spec);
        eprintln!("{} {}", r.status, r.summary);
        reports.push(r);
    }

    eprintln!("\ncipher / KDF matrix ...");
    let matrix = run_matrix(&ctx);
    eprintln!("elements KeePassXC does not keep ...");
    let foreign = run_foreign(&ctx);
    eprintln!("damaged files ...");
    let damaged = run_damaged(&ctx);
    eprintln!("editing ...");
    let editing = run_edit(&ctx);
    eprintln!("fuzzing (15 s) ...");
    let fuzz = run_fuzz_section(&ctx, Duration::from_secs(15), 0x6d696b6f736869);

    let extra = [&matrix, &foreign, &damaged, &editing, &fuzz];
    let report = render_report(&ctx, &reports, &extra, started.elapsed());
    let path = out_dir().join("report.md");
    if let Err(e) = std::fs::write(&path, &report) {
        eprintln!("cannot write {}: {e}", path.display());
    }
    for sec in extra {
        eprintln!("{:44} {}", sec.title, sec.status());
    }
    eprintln!("\nreport: {}", path.display());

    let failed = reports.iter().any(|r| r.status == "FAIL") || extra.iter().any(|s| !s.ok);
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn check_fixture(ctx: &Ctx, spec: &FixtureSpec) -> FixtureReport {
    let path = ctx.fixtures.join(&spec.file);
    let mut report = FixtureReport {
        file: spec.file.clone(),
        made_by: spec.made_by.clone(),
        summary: String::new(),
        status: "ok",
        sections: Vec::new(),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) if spec.optional => {
            report.status = "skip";
            report.summary = "not made yet (see tests/fixtures/README.md)".into();
            return report;
        }
        Err(e) => {
            report.status = "FAIL";
            report.summary = format!("cannot read: {e}");
            return report;
        }
    };
    let key = ctx.key(spec);

    // ---- header
    let mut sec = Section::new("Header");
    match header::read(&bytes) {
        Ok(h) => {
            sec.note(format!("{h}"));
            if h.version != spec.format {
                sec.fail(format!(
                    "expected KDBX {}, file is {}",
                    spec.format, h.version
                ));
            }
            if h.cipher != spec.cipher {
                sec.fail(format!("expected {}, file uses {}", spec.cipher, h.cipher));
            }
            if kdf_name(&h.kdf) != spec.kdf {
                sec.fail(format!("expected {}, file uses {}", spec.kdf, h.kdf));
            }
        }
        Err(e) => sec.fail(format!("cannot read header: {e}")),
    }
    report.sections.push(sec);

    // ---- open
    let mut sec = Section::new("Open");
    let t = Instant::now();
    let db = match open(&bytes, &key) {
        Ok(db) => db,
        Err(e) => {
            sec.fail(format!("cannot open it: {e}"));
            report.sections.push(sec);
            report.status = "FAIL";
            report.summary = "cannot open".into();
            return report;
        }
    };
    let open_time = t.elapsed();
    sec.note(format!(
        "opened in {} ({} entries, {} groups)",
        ms(open_time),
        db.entries().len(),
        db.groups().len()
    ));
    report.sections.push(sec);

    // ---- read vs KeePassXC
    let mut theirs: Option<Canon> = None;
    if let Some(kxc) = &ctx.kxc {
        let mut sec = Section::new("Read: kdbx vs KeePassXC");
        match (
            kxc.export_xml(&path, &key)
                .and_then(|x| canon::parse(&x, Source::Export)),
            our_view(&db),
        ) {
            (Ok(c), Ok(ours)) => {
                let diffs = canon::diff(&c, &ours);
                let labels = [&c.labels, &ours.labels];
                let sorted = sort_diffs(&diffs, &c);
                if sorted.real.is_empty() {
                    sec.note(format!("identical ({} records compared)", c.records.len()));
                }
                let mut sub = Section::new("");
                report_diffs(&mut sub, &sorted, &labels, "", true);
                for l in sub.lines {
                    sec.lines.push(
                        l.replace("- LOST    ", "- ONLY IN KEEPASSXC")
                            .replace("- ADDED   ", "- ONLY IN KDBX"),
                    );
                }
                sec.ok &= sub.ok;
                theirs = Some(c);
            }
            (Err(e), _) => sec.fail(format!("keepassxc-cli export failed: {e}")),
            (_, Err(e)) => sec.fail(format!("cannot parse our own export: {e}")),
        }
        attachments_vs_kxc(kxc, &path, &key, &db, &mut sec, "original");
        totp_vs_kxc(kxc, &path, &key, &db, &mut sec);
        report.sections.push(sec);
    }

    // ---- wrong key
    let mut sec = Section::new("Wrong key");
    let wrong = Key {
        password: Some(
            key.password
                .clone()
                .map(|p| format!("{p}-wrong"))
                .unwrap_or_else(|| "wrong".into()),
        ),
        keyfile: key.keyfile.clone(),
    };
    match open(&bytes, &wrong) {
        Ok(_) => sec.fail("opened with a wrong password!"),
        Err(OpenError::Failed(kdbx::Error::WrongKey)) => sec.note("reported as wrong key"),
        Err(e) => sec.fail(format!("wrong password reported as: {e}")),
    }
    report.sections.push(sec);

    // ---- round trip
    let mut sec = Section::new("Round trip: open → save → reopen");
    let t = Instant::now();
    let saved = match save(&db, &key) {
        Ok(s) => s,
        Err(e) => {
            sec.fail(format!("save failed: {e}"));
            report.sections.push(sec);
            report.status = "FAIL";
            report.summary = "save failed".into();
            return report;
        }
    };
    let save_time = t.elapsed();
    let saved_name = Path::new(&spec.file)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let saved_path = out_dir().join("saved").join(&saved_name);
    let _ = std::fs::write(&saved_path, &saved);
    match header::read(&saved) {
        Ok(h) => {
            sec.note(format!("saved in {} as {h}", ms(save_time)));
            let before = header::read(&bytes).ok();
            if let Some(b) = before {
                sec.check(
                    h.cipher == b.cipher && h.compression == b.compression,
                    format!(
                        "cipher and compression kept: {}, {}",
                        h.cipher, h.compression
                    ),
                );
                let same_kdf = match (&h.kdf, &b.kdf) {
                    (header::Kdf::Aes { rounds: a, .. }, header::Kdf::Aes { rounds: c, .. }) => {
                        a == c
                    }
                    (x, y) => x == y,
                };
                sec.check(same_kdf, format!("KDF parameters kept: {}", h.kdf));
            }
        }
        Err(e) => sec.fail(format!("saved file has a bad header: {e}")),
    }

    let reopened = match open(&saved, &key) {
        Ok(db) => db,
        Err(e) => {
            sec.fail(format!("cannot reopen the saved file: {e}"));
            report.sections.push(sec);
            report.status = "FAIL";
            report.summary = "saved file unreadable".into();
            return report;
        }
    };
    same_tree(&mut sec, &db, &reopened, "kdbx before vs after");

    // the XML actually stored in the file, before and after
    match (raw_xml(&bytes, &key), raw_xml(&saved, &key)) {
        (Ok(a), Ok(b)) => match (canon::parse(&a, Source::Raw), canon::parse(&b, Source::Raw)) {
            (Ok(ca), Ok(cb)) => {
                let diffs = canon::diff(&ca, &cb);
                let labels = [&ca.labels];
                let sorted = sort_diffs(&diffs, &ca);
                if sorted.real.is_empty() {
                    sec.note(format!(
                        "stored XML: nothing lost ({} records compared)",
                        ca.records.len()
                    ));
                }
                report_diffs(&mut sec, &sorted, &labels, "stored XML", true);
            }
            (Err(e), _) | (_, Err(e)) => sec.fail(format!("cannot parse stored XML: {e}")),
        },
        (Err(e), _) | (_, Err(e)) => sec.fail(format!("cannot decrypt stored XML: {e}")),
    }

    // what KeePassXC sees in the saved file
    if let (Some(kxc), Some(theirs)) = (&ctx.kxc, &theirs) {
        match kxc
            .export_xml(&saved_path, &key)
            .and_then(|x| canon::parse(&x, Source::Export))
        {
            Ok(after_kxc) => {
                let diffs = canon::diff(theirs, &after_kxc);
                let labels = [&theirs.labels];
                let sorted = sort_diffs(&diffs, theirs);
                if sorted.real.is_empty() {
                    sec.note("KeePassXC reads the saved file the same as the original");
                }
                report_diffs(&mut sec, &sorted, &labels, "KeePassXC sees", true);
            }
            Err(e) => sec.fail(format!("KeePassXC cannot open the saved file: {e}")),
        }
        attachments_vs_kxc(kxc, &saved_path, &key, &db, &mut sec, "saved");
    }
    report.sections.push(sec);

    let failures: Vec<&str> = report
        .sections
        .iter()
        .filter(|s| !s.ok)
        .map(|s| s.title.as_str())
        .collect();
    report.status = if failures.is_empty() { "ok" } else { "FAIL" };
    report.summary = if failures.is_empty() {
        format!("opened in {}, saved in {}", ms(open_time), ms(save_time))
    } else {
        format!("problems in: {}", failures.join("; "))
    };
    report
}

fn attachments_vs_kxc(
    kxc: &Kxc,
    path: &Path,
    key: &Key,
    db: &Database,
    sec: &mut Section,
    which: &str,
) {
    let ours = model::attachments(db);
    let entries = model::cli_entry_paths(db);
    let mut checked = 0;
    let mut skipped_history = 0;
    for (att_path, info) in &ours {
        if att_path.contains("/History/") {
            skipped_history += 1;
            continue;
        }
        let Some((entry_path, name)) = att_path.rsplit_once("/Binary[") else {
            continue;
        };
        let name = name.trim_end_matches(']');
        let Some(cli) = entries.get(entry_path) else {
            continue;
        };
        match kxc.attachment(path, key, cli, name, &out_dir().join("attachment.tmp")) {
            Ok(data) => {
                checked += 1;
                if data.len() != info.len || canon::short_hash(&data) != info.hash {
                    sec.fail(format!("attachment \"{name}\" of \"{cli}\" differs in the {which} file ({} vs {} bytes)", data.len(), info.len));
                }
            }
            Err(e) => sec.fail(format!("KeePassXC cannot export attachment \"{name}\" of \"{cli}\" from the {which} file: {e}")),
        }
    }
    sec.note(format!(
        "attachments ({which} file): {checked} compared byte for byte with KeePassXC{}",
        if skipped_history > 0 {
            format!(" ({skipped_history} in history skipped: keepassxc-cli cannot export those)")
        } else {
            String::new()
        }
    ));
}

fn totp_vs_kxc(kxc: &Kxc, path: &Path, key: &Key, db: &Database, sec: &mut Section) {
    use mikoshi_core::totp::{self, Totp};
    let entries = model::cli_entry_paths(db);
    let mut checked = 0;
    for e in db.entries() {
        let fields: HashMap<String, String> = e
            .fields()
            .map(|f| (f.key().into_owned(), f.value().into_owned()))
            .collect();
        let get = |k: &str| fields.get(k).map(String::as_str);
        if !totp::has_totp(get) {
            continue;
        }
        let Some(id) = e.uuid() else { continue };
        let Some((_, cli)) = entries
            .iter()
            .find(|(p, _)| p.ends_with(&format!("Entry[{}]", id.to_base64())))
        else {
            continue;
        };
        let ours = match Totp::from_fields(get) {
            Ok(t) => t,
            Err(err) => {
                sec.fail(format!("TOTP of \"{cli}\": cannot parse it ({err:?})"));
                continue;
            }
        };
        // keepassxc-cli takes a moment (it opens the database), so its code
        // belongs to some instant between `before` and `after`. Compare with
        // our codes for both ends: a 30 s boundary may fall inside the call.
        let unix_now = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };
        let before = unix_now();
        let theirs = kxc
            .totp(path, key, cli)
            .unwrap_or_else(|e| format!("error: {e}"));
        let after = unix_now();
        let theirs_code = theirs
            .lines()
            .last()
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        let mut mine: Vec<String> = [before, after]
            .iter()
            .map(|&t| ours.code_at(t).code)
            .collect();
        mine.dedup();
        checked += 1;
        if !mine.contains(&theirs_code) {
            sec.fail(format!(
                "TOTP of \"{cli}\" differs: Mikoshi {}, KeePassXC {theirs_code}",
                mine.join(" or ")
            ));
        }
    }
    if checked > 0 {
        sec.note(format!("TOTP: {checked} codes compared with KeePassXC"));
    }
}

// ---------------------------------------------------------------- cipher / KDF matrix

fn fast_kdfs() -> Vec<(&'static str, Kdf)> {
    let argon = |id| Kdf::Argon2 {
        id,
        memory: 16 * 1024 * 1024,
        iterations: 2,
        parallelism: 2,
        version: 0x13,
    };
    vec![
        ("AES-KDF", Kdf::Aes { rounds: 100_000 }),
        ("Argon2d", argon(false)),
        ("Argon2id", argon(true)),
    ]
}

fn run_matrix(ctx: &Ctx) -> Section {
    let mut sec = Section::new("Cipher / KDF matrix written by kdbx");
    let Some(spec) = ctx.spec("kxc/kdbx41-pw.kdbx") else {
        sec.fail("base fixture kxc/kdbx41-pw.kdbx missing from the manifest");
        return sec;
    };
    let key = ctx.key(&spec);
    let base_path = ctx.fixtures.join(&spec.file);
    let Ok(bytes) = std::fs::read(&base_path) else {
        sec.fail("base fixture missing; run tests/fixtures/generate.py");
        return sec;
    };
    let Ok(db) = open(&bytes, &key) else {
        sec.fail("cannot open base fixture");
        return sec;
    };
    let base_export = ctx
        .kxc
        .as_ref()
        .and_then(|k| k.export_xml(&base_path, &key).ok())
        .and_then(|x| canon::parse(&x, Source::Export).ok());

    sec.note(
        "| cipher | KDF | kdbx reopens, same tree | KeePassXC opens | KeePassXC sees same data |",
    );
    sec.note("|---|---|---|---|---|");
    let mut variant = match open(&bytes, &key) {
        Ok(v) => v,
        Err(e) => {
            sec.fail(format!("cannot open base fixture: {e}"));
            return sec;
        }
    };
    for (cipher_name, cipher) in [
        ("AES-256", Cipher::Aes256),
        ("Twofish", Cipher::Twofish),
        ("ChaCha20", Cipher::ChaCha20),
    ] {
        for (kdf_name, kdf) in fast_kdfs() {
            let mut s = variant.settings();
            s.cipher = cipher;
            s.kdf = kdf;
            variant.set_settings(&s);
            let file = out_dir().join("variants").join(format!(
                "{}-{}.kdbx",
                cipher_name.to_lowercase(),
                kdf_name.to_lowercase()
            ));
            let saved = match save(&variant, &key) {
                Ok(s) => s,
                Err(e) => {
                    sec.fail(format!(
                        "| {cipher_name} | {kdf_name} | save failed: {e} | | |"
                    ));
                    continue;
                }
            };
            let _ = std::fs::write(&file, &saved);
            let reopen = match open(&saved, &key) {
                Ok(r) if r.document() == db.document() && r.binaries() == db.binaries() => {
                    "yes".to_string()
                }
                Ok(_) => "data differs".to_string(),
                Err(e) => e.to_string(),
            };
            let (opens, same) = match (&ctx.kxc, &base_export) {
                (Some(kxc), Some(base)) => match kxc
                    .export_xml(&file, &key)
                    .and_then(|x| canon::parse(&x, Source::Export))
                {
                    Ok(c) => {
                        let d = canon::diff(base, &c);
                        let sorted = sort_diffs(&d, base);
                        let verdict = if sorted.real.is_empty() {
                            "yes".to_string()
                        } else {
                            format!("{} differences", sorted.real.len())
                        };
                        ("yes".to_string(), verdict)
                    }
                    Err(e) => (format!("no: {e}"), "-".to_string()),
                },
                _ => ("(no keepassxc-cli)".to_string(), "-".to_string()),
            };
            let row = format!("| {cipher_name} | {kdf_name} | {reopen} | {opens} | {same} |");
            sec.check(
                reopen == "yes" && opens.starts_with("yes") && same == "yes"
                    || reopen == "yes" && opens == "(no keepassxc-cli)",
                row,
            );
        }
    }
    sec
}

// ---------------------------------------------------------------- elements KeePassXC does not keep

/// Test databases for M0 issues K6–K9. KeePassXC drops unknown elements when
/// it imports XML, so these are written by our own minimal writer
/// (src/writer.rs) from the fixture content, once, into tests/fixtures/foreign/.
fn foreign_cases(
    content: &str,
) -> Vec<(&'static str, &'static str, String, Vec<writer::Injection>)> {
    use writer::{Injection, Target};
    vec![
        (
            "k6-k7-unknown-elements.kdbx",
            // the content already has KeePass 2's MasterKeyChangeForceOnce (K7)
            "K6, K7: unknown elements in Meta, Root, Entry, AutoType + KeePass 2's MasterKeyChangeForceOnce",
            content.to_string(),
            vec![
                Injection {
                    target: Target::Meta,
                    xml: "<MikoshiUnknownMeta>meta-level unknown element</MikoshiUnknownMeta>",
                },
                Injection {
                    target: Target::Root,
                    xml: "<MikoshiUnknownRoot>root-level unknown element</MikoshiUnknownRoot>",
                },
                Injection {
                    target: Target::Entry("Full Fields"),
                    xml: "<MikoshiUnknownEntry Flag=\"1\"><Nested>entry-level unknown element</Nested></MikoshiUnknownEntry>",
                },
                Injection {
                    target: Target::AutoType("AutoType Custom"),
                    xml: "<MikoshiUnknownAutoType>auto-type-level unknown element</MikoshiUnknownAutoType>",
                },
            ],
        ),
        (
            "k8-unknown-in-group.kdbx",
            "K8: unknown element directly inside a Group",
            content.to_string(),
            vec![Injection {
                target: Target::Group("General"),
                xml: "<MikoshiUnknownGroup>group-level unknown element</MikoshiUnknownGroup>",
            }],
        ),
        (
            "k9-bad-timestamp.kdbx",
            "K9: malformed (too short) timestamp",
            content.replacen(
                "<LastAccessTime>2024-01-10T08:00:00Z</LastAccessTime>",
                "<LastAccessTime>AAAA</LastAccessTime>",
                1,
            ),
            vec![],
        ),
    ]
}

fn run_foreign(ctx: &Ctx) -> Section {
    let mut sec = Section::new("Elements KeePassXC does not keep (K6–K9)");
    let Ok(content) = std::fs::read_to_string(ctx.fixtures.join("src/content-41.xml")) else {
        sec.fail("tests/fixtures/src/content-41.xml missing; run tests/fixtures/generate.py");
        return sec;
    };
    let password = ctx.manifest.password.clone();
    let key = Key {
        password: Some(password.clone()),
        keyfile: None,
    };
    let dir = ctx.fixtures.join("foreign");
    let _ = std::fs::create_dir_all(&dir);
    let saved_dir = out_dir().join("foreign");
    let _ = std::fs::create_dir_all(&saved_dir);

    for (name, what, xml, injections) in foreign_cases(&content) {
        let file = dir.join(name);
        let bytes = match std::fs::read(&file) {
            Ok(b) => b,
            Err(_) => match writer::write(&xml, &password, &injections) {
                Ok(b) => {
                    let _ = std::fs::write(&file, &b);
                    sec.note(format!("{what}: wrote tests/fixtures/foreign/{name}"));
                    b
                }
                Err(e) => {
                    sec.fail(format!("{what}: cannot write test file: {e}"));
                    continue;
                }
            },
        };
        let kxc_opens = ctx.kxc.as_ref().map(|k| k.export_xml(&file, &key).is_ok());
        let db = match open(&bytes, &key) {
            Ok(db) => db,
            Err(e) => {
                sec.fail(format!("{what}: cannot open it: {e}"));
                continue;
            }
        };
        let saved = match save(&db, &key) {
            Ok(s) => s,
            Err(e) => {
                sec.fail(format!("{what}: save failed: {e}"));
                continue;
            }
        };
        let saved_path = saved_dir.join(name);
        let _ = std::fs::write(&saved_path, &saved);
        match open(&saved, &key) {
            Ok(again) => same_tree(&mut sec, &db, &again, what),
            Err(e) => sec.fail(format!("{what}: cannot reopen the saved file: {e}")),
        }
        match (raw_xml(&bytes, &key), raw_xml(&saved, &key)) {
            (Ok(a), Ok(b)) => {
                for marker in ["MikoshiUnknown", "MasterKeyChangeForceOnce", ">AAAA<"] {
                    let n = a.matches(marker).count();
                    if n > 0 {
                        sec.check(
                            b.matches(marker).count() == n,
                            format!("{what}: {marker} ×{n} still in the saved file"),
                        );
                    }
                }
                match (canon::parse(&a, Source::Raw), canon::parse(&b, Source::Raw)) {
                    (Ok(ca), Ok(cb)) => {
                        let diffs = canon::diff(&ca, &cb);
                        let sorted = sort_diffs(&diffs, &ca);
                        report_diffs(&mut sec, &sorted, &[&ca.labels], &format!("{what}:"), true);
                    }
                    (Err(e), _) | (_, Err(e)) => sec.fail(format!("{what}: cannot parse XML: {e}")),
                }
            }
            (Err(e), _) | (_, Err(e)) => sec.fail(format!("{what}: cannot decrypt: {e}")),
        }
        if let Some(kxc) = &ctx.kxc {
            let after = kxc.export_xml(&saved_path, &key).is_ok();
            match kxc_opens {
                Some(true) => sec.check(
                    after,
                    format!("{what}: KeePassXC opens the original and the saved file"),
                ),
                _ => sec.note(format!(
                    "{what}: KeePassXC refuses the original{}",
                    if after {
                        ", opens the saved file"
                    } else {
                        " and the saved file"
                    }
                )),
            }
        }
    }
    sec
}

// ---------------------------------------------------------------- damaged files

fn run_damaged(ctx: &Ctx) -> Section {
    let mut sec = Section::new("Damaged files and wrong keys");
    sec.note("| file | damage | reported as |");
    sec.note("|---|---|---|");
    for name in ["kxc/kdbx41-pw.kdbx", "kxc/kdbx31-pw.kdbx"] {
        let Some(spec) = ctx.spec(name) else { continue };
        let key = ctx.key(&spec);
        let Ok(bytes) = std::fs::read(ctx.fixtures.join(name)) else {
            continue;
        };
        let kdbx3 = spec.format == "3.1";
        let header_len = header_length(&bytes).unwrap_or(200);
        // expected: Some(true) wrong key, Some(false) damaged, None either
        let mut cases: Vec<(&str, Vec<u8>, Key, Option<bool>)> = Vec::new();
        cases.push((
            "wrong password",
            bytes.clone(),
            Key {
                password: Some("not-the-password".into()),
                keyfile: None,
            },
            Some(true),
        ));
        cases.push((
            "no password, key file only",
            bytes.clone(),
            Key {
                password: None,
                keyfile: Some(ctx.fixtures.join("keys/binary-32.key")),
            },
            Some(true),
        ));
        let mut t = bytes.clone();
        t.truncate(bytes.len() / 2);
        cases.push(("truncated to half", t, key.clone(), Some(false)));
        let mut t = bytes.clone();
        t.truncate(header_len.saturating_sub(10));
        cases.push(("truncated inside the header", t, key.clone(), Some(false)));
        let mut t = bytes.clone();
        t.truncate(header_len + 20);
        cases.push((
            "truncated right after the header",
            t,
            key.clone(),
            Some(false),
        ));
        let mut t = bytes.clone();
        if let Some(b) = t.get_mut(header_len.saturating_sub(20)) {
            *b ^= 0x01;
        }
        // KDBX 3.1 has no checksum on the header outside the encrypted part:
        // a flipped seed or IV is indistinguishable from a wrong key
        cases.push((
            "one bit flipped in the header",
            t,
            key.clone(),
            if kdbx3 { None } else { Some(false) },
        ));
        let mut t = bytes.clone();
        if let Some(b) = t.get_mut(header_len + 80) {
            *b ^= 0x01;
        }
        cases.push((
            "one bit flipped at the start of the payload",
            t,
            key.clone(),
            Some(false),
        ));
        let mut t = bytes.clone();
        let n = t.len();
        if let Some(b) = t.get_mut(n - 40) {
            *b ^= 0x01;
        }
        cases.push(("one bit flipped near the end", t, key.clone(), Some(false)));
        for (what, data, k, expect_wrong_key) in cases {
            let result = match open(&data, &k) {
                Ok(damaged_db) => {
                    // only acceptable if the damage did not touch the content
                    let same = open(&bytes, &key)
                        .map(|orig| orig.document() == damaged_db.document())
                        .unwrap_or(false);
                    sec.check(same, format!("| {name} | {what} | opened{} |", if same {
                        "; content identical to the original (the bit is in padding the format does not check)"
                    } else {
                        " WITH DIFFERENT CONTENT"
                    }));
                    continue;
                }
                Err(e) => e,
            };
            let is_wrong_key = matches!(result, OpenError::Failed(kdbx::Error::WrongKey));
            let is_damaged = matches!(result, OpenError::Failed(kdbx::Error::Corrupt(_)));
            let ok = match expect_wrong_key {
                Some(true) => is_wrong_key,
                Some(false) => is_damaged,
                None => is_wrong_key || is_damaged,
            };
            sec.check(ok, format!("| {name} | {what} | {result} |"));
        }
    }
    let random: Vec<u8> = (0..4096u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    for (what, data, expected) in [
        ("empty file", Vec::new(), "not a KeePass file"),
        ("random bytes", random, "not a KeePass file"),
        ("KeePass 1 (.kdb) signature", kdb_stub(), "KeePass 1 file"),
    ] {
        let result = match open(
            &data,
            &Key {
                password: Some("x".into()),
                keyfile: None,
            },
        ) {
            Ok(_) => "OPENED (!)".to_string(),
            Err(e) => e.to_string(),
        };
        sec.check(result == expected, format!("| - | {what} | {result} |"));
    }
    sec
}

fn kdb_stub() -> Vec<u8> {
    let mut v = vec![0x03, 0xd9, 0xa2, 0x9a, 0x65, 0xfb, 0x4b, 0xb5];
    v.resize(124 + 64, 0);
    v
}

fn header_length(bytes: &[u8]) -> Option<usize> {
    let major = u16::from_le_bytes(bytes.get(10..12)?.try_into().ok()?);
    let wide = major >= 4;
    let mut pos = 12;
    loop {
        let kind = *bytes.get(pos)?;
        let (len, skip) = if wide {
            (
                u32::from_le_bytes(bytes.get(pos + 1..pos + 5)?.try_into().ok()?) as usize,
                5,
            )
        } else {
            (
                usize::from(u16::from_le_bytes(
                    bytes.get(pos + 1..pos + 3)?.try_into().ok()?,
                )),
                3,
            )
        };
        pos += skip + len;
        if kind == 0 {
            return Some(pos);
        }
    }
}

// ---------------------------------------------------------------- editing

fn run_edit(ctx: &Ctx) -> Section {
    let mut sec = Section::new("Edit with kdbx, read back with KeePassXC");
    let Some(spec) = ctx.spec("kxc/kdbx41-pw.kdbx") else {
        sec.fail("base fixture missing");
        return sec;
    };
    let key = ctx.key(&spec);
    let Ok(bytes) = std::fs::read(ctx.fixtures.join(&spec.file)) else {
        sec.fail("base fixture missing");
        return sec;
    };
    let Ok(mut db) = open(&bytes, &key) else {
        sec.fail("cannot open base fixture");
        return sec;
    };

    let find = |db: &Database, title: &str| {
        db.entries()
            .into_iter()
            .find(|e| e.get("Title").as_deref() == Some(title))
            .and_then(|e| e.uuid())
    };
    let find_group = |db: &Database, name: &str| {
        db.groups()
            .into_iter()
            .find(|g| g.name() == name)
            .and_then(|g| g.uuid())
    };
    let (Some(full), Some(expired), Some(general)) = (
        find(&db, "Full Fields"),
        find(&db, "Expired"),
        find_group(&db, "General"),
    ) else {
        sec.fail("fixture content not as expected");
        return sec;
    };
    let recycle = db.recycle_bin().and_then(|g| g.uuid());
    let original = open(&bytes, &key).ok();

    // 1. edit an entry, keeping the old version in history (what Mikoshi will do)
    let history_before = db.entry(full).map(|e| e.history().count()).unwrap_or(0);
    if let Some(mut e) = db.entry_mut(full) {
        e.push_history();
        e.set_field("Title", "Full Fields (edited)", false);
        e.set_field("Password", "new-password-✓", true);
        e.set_field("New Secret", "added by kdbx", true);
        e.touch();
    }
    // 2. new entry with an attachment, in a new group
    let new_group = db.add_group(general, "Added Group");
    let new_entry = new_group.and_then(|g| db.add_entry(g));
    if let Some(id) = new_entry {
        if let Some(mut e) = db.entry_mut(id) {
            e.set_field("Title", "Added Entry", false);
            e.set_field("UserName", "added-user", false);
            e.set_field("Password", "added-password", true);
        }
        db.add_attachment(id, "added.txt", b"attachment added by kdbx", false);
    }
    // 3. move an entry into the recycle bin
    if let Some(bin) = recycle {
        db.move_entry(expired, bin);
    }

    let saved = match save(&db, &key) {
        Ok(s) => s,
        Err(e) => {
            sec.fail(format!("save failed: {e}"));
            return sec;
        }
    };
    let path = out_dir().join("saved").join("edited.kdbx");
    let _ = std::fs::write(&path, &saved);

    // the edits touch only their own nodes: without the edited, moved and
    // added ones, both trees must be identical
    if let (Some(orig), Ok(after)) = (&original, open(&saved, &key)) {
        let mut touched = vec![full, expired];
        touched.extend(new_group);
        let a = model::without(orig.document(), &touched);
        let b = model::without(after.document(), &touched);
        let diffs = model::tree_diff(&a, &b, 20);
        sec.check(
            diffs.is_empty(),
            "everything the edits did not touch is identical node by node",
        );
        for d in &diffs {
            sec.fail(format!("  untouched node changed: {d}"));
        }
    }

    let Some(kxc) = &ctx.kxc else {
        sec.note("keepassxc-cli not found; only checked that kdbx reopens it");
        if open(&saved, &key).is_err() {
            sec.fail("kdbx cannot reopen its own edited file");
        }
        return sec;
    };
    let export = match kxc
        .export_xml(&path, &key)
        .and_then(|x| canon::parse(&x, Source::Export))
    {
        Ok(c) => c,
        Err(e) => {
            sec.fail(format!("KeePassXC cannot open the edited file: {e}"));
            return sec;
        }
    };
    let find_record = |suffix: &str| {
        export
            .records
            .iter()
            .find(|(k, _)| k.ends_with(suffix))
            .map(|(k, v)| (k.clone(), v.clone()))
    };
    let mut expect = |what: &str, ok: bool| {
        if ok {
            sec.note(format!("ok: {what}"))
        } else {
            sec.fail(format!("NOT OK: {what}"))
        }
    };
    let full_key = format!("Entry[{}]", full.to_base64());
    expect(
        "edited title",
        find_record(&format!("{full_key}/String[Title]/Value")).map(|(_, v)| v)
            == Some("Full Fields (edited)".into()),
    );
    expect(
        "edited password",
        find_record(&format!("{full_key}/String[Password]/Value")).map(|(_, v)| v)
            == Some("new-password-✓".into()),
    );
    expect(
        "new protected custom field",
        find_record(&format!("{full_key}/String[New Secret]/Value@Protected")).is_some(),
    );
    let history_after = export
        .records
        .keys()
        .filter(|k| {
            k.contains(&full_key) && k.contains("/History/Entry#") && k.ends_with("/String[Title]")
        })
        .count();
    expect(
        &format!(
            "history grew from {history_before} to {}",
            history_before + 1
        ),
        history_after == history_before + 1,
    );
    if let Some(id) = new_entry {
        let k = format!("Entry[{}]", id.to_base64());
        expect(
            "new entry inside the new group",
            export
                .records
                .keys()
                .any(|p| p.contains("/Group[") && p.ends_with(&format!("{k}/String[UserName]"))),
        );
        match kxc.attachment(
            &path,
            &key,
            "General/Added Group/Added Entry",
            "added.txt",
            &out_dir().join("attachment.tmp"),
        ) {
            Ok(d) => expect(
                "attachment of the new entry",
                d == b"attachment added by kdbx",
            ),
            Err(e) => expect(&format!("attachment of the new entry ({e})"), false),
        }
    }
    if let Some(bin) = recycle {
        let exp = format!("Group[{}]/Entry[{}]", bin.to_base64(), expired.to_base64());
        expect(
            "entry moved to the recycle bin",
            export.records.keys().any(|k| k.ends_with(&exp)),
        );
        let prev = export
            .records
            .iter()
            .find(|(k, _)| k.ends_with(&format!("{exp}/PreviousParentGroup")))
            .map(|(_, v)| v.clone());
        expect(
            "PreviousParentGroup set on the moved entry",
            prev == Some(general.to_base64()),
        );
    }
    sec
}

// ---------------------------------------------------------------- fuzzing

fn fuzz_corpus(ctx: &Ctx) -> Result<kdbx::fuzz::Corpus, String> {
    let mut dbs = Vec::new();
    let mut xml3 = Vec::new();
    for spec in &ctx.manifest.fixture {
        let Ok(bytes) = std::fs::read(ctx.fixtures.join(&spec.file)) else {
            continue;
        };
        let key = ctx.key(spec);
        if spec.format == "3.1"
            && xml3.len() < 2
            && let Ok(x) = raw_xml(&bytes, &key)
        {
            xml3.push(x.into_bytes());
        }
        if dbs.len() < 3
            && let Ok(db) = open(&bytes, &key)
        {
            dbs.push(db);
        }
    }
    let foreign = ctx.fixtures.join("foreign");
    let pw = Key {
        password: Some(ctx.manifest.password.clone()),
        keyfile: None,
    };
    if let Ok(dir) = std::fs::read_dir(&foreign) {
        for f in dir.flatten() {
            if let Ok(bytes) = std::fs::read(f.path())
                && let Ok(db) = open(&bytes, &pw)
            {
                dbs.push(db);
            }
        }
    }
    if dbs.is_empty() {
        return Err("no fixture opens; run tests/fixtures/generate.py".into());
    }
    let keyfiles = [
        "keys/xml-v1.key",
        "keys/xml-v2.keyx",
        "keys/hex-64.key",
        "keys/binary-32.key",
    ]
    .iter()
    .filter_map(|k| std::fs::read(ctx.fixtures.join(k)).ok())
    .collect();
    let refs: Vec<&Database> = dbs.iter().collect();
    Ok(kdbx::fuzz::Corpus::from_databases(
        &refs,
        kdbx::Key::new().with_password("fuzz-corpus"),
        xml3,
        keyfiles,
    ))
}

fn run_fuzz_section(ctx: &Ctx, budget: Duration, seed: u64) -> Section {
    let mut sec = Section::new("Fuzzing: mutated files and payloads");
    let corpus = match fuzz_corpus(ctx) {
        Ok(c) => c,
        Err(e) => {
            sec.fail(e);
            return sec;
        }
    };
    let out = kdbx::fuzz::run(&corpus, seed, kdbx::fuzz::Budget::Time(budget));
    sec.note(format!(
        "seed {seed:#x}: {} cases in {} s, {} accepted and round-tripped",
        out.cases,
        budget.as_secs(),
        out.accepted
    ));
    for f in &out.failures {
        sec.fail(f.clone());
    }
    if out.failures.is_empty() {
        sec.note("no panics, no round-trip differences");
    }
    sec
}

fn run_fuzz(args: &[String]) -> ExitCode {
    let secs = args.first().and_then(|s| s.parse().ok()).unwrap_or(60);
    let seed = args.get(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
    });
    let ctx = match load_ctx() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let sec = run_fuzz_section(&ctx, Duration::from_secs(secs), seed);
    for l in &sec.lines {
        println!("{l}");
    }
    if sec.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---------------------------------------------------------------- report

fn render_report(
    ctx: &Ctx,
    reports: &[FixtureReport],
    extra: &[&Section],
    elapsed: Duration,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# kdbx-compat report\n");
    let _ = writeln!(s, "- reader / writer: kdbx crate (crates/kdbx)");
    let _ = writeln!(
        s,
        "- reference: {}",
        ctx.kxc
            .as_ref()
            .map(|k| k.version())
            .unwrap_or_else(|| "keepassxc-cli not found — cross-checks skipped".into())
    );
    let _ = writeln!(
        s,
        "- build: {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    let _ = writeln!(s, "- total time: {:.1} s\n", elapsed.as_secs_f64());
    let _ = writeln!(
        s,
        "Saved files are in `target/kdbx-compat/saved/`, cipher/KDF variants in `target/kdbx-compat/variants/` — open them in KeePass 2 for the manual check.\n"
    );

    let _ = writeln!(s, "## Summary\n");
    let _ = writeln!(s, "| check | made by | result |");
    let _ = writeln!(s, "|---|---|---|");
    for r in reports {
        let _ = writeln!(
            s,
            "| `{}` | {} | {} {} |",
            r.file, r.made_by, r.status, r.summary
        );
    }
    for sec in extra {
        let _ = writeln!(s, "| {} | | {} |", sec.title, sec.status());
    }

    let _ = writeln!(s, "\n## M0 issues (keepass-rs)\n");
    let _ = writeln!(
        s,
        "Found in keepass-rs during M0 (docs/m0-report.md). The kdbx crate must not have them; each one is checked here:\n"
    );
    for (id, text, check) in M0_ISSUES {
        let _ = writeln!(s, "- **{id}** {text} — checked by: {check}");
    }

    for r in reports.iter().filter(|r| !r.sections.is_empty()) {
        let _ = writeln!(s, "\n## `{}`\n", r.file);
        for sec in &r.sections {
            let _ = writeln!(s, "**{}** — {}\n", sec.title, sec.status());
            for l in &sec.lines {
                let _ = writeln!(s, "    {l}");
            }
            let _ = writeln!(s);
        }
    }
    for sec in extra {
        let _ = writeln!(s, "\n## {} — {}\n", sec.title, sec.status());
        for l in &sec.lines {
            if l.starts_with('|') {
                let _ = writeln!(s, "{l}");
            } else {
                let _ = writeln!(s, "- {l}");
            }
        }
    }
    s
}

// ---------------------------------------------------------------- check mode

fn run_check(args: &[String]) -> ExitCode {
    let mut file = None;
    let mut keyfile = None;
    let mut no_password = false;
    let mut password_stdin = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--keyfile" => keyfile = it.next().map(PathBuf::from),
            "--no-password" => no_password = true,
            "--password-stdin" => password_stdin = true,
            other => file = Some(PathBuf::from(other)),
        }
    }
    let Some(file) = file else {
        eprintln!(
            "usage: kdbx-compat check <file.kdbx> [--keyfile <path>] [--no-password | --password-stdin]"
        );
        return ExitCode::FAILURE;
    };
    let password = if no_password {
        None
    } else if password_stdin {
        let mut line = String::new();
        if let Err(e) = std::io::stdin().read_line(&mut line) {
            eprintln!("cannot read password: {e}");
            return ExitCode::FAILURE;
        }
        Some(line.trim_end_matches(['\r', '\n']).to_string())
    } else {
        match rpassword::prompt_password("Master password: ") {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("cannot read password: {e}");
                return ExitCode::FAILURE;
            }
        }
    };
    let key = Key { password, keyfile };
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {}: {e}", file.display());
            return ExitCode::FAILURE;
        }
    };
    println!("Nothing is written to disk. Output lists paths only, never values.\n");
    match header::read(&bytes) {
        Ok(h) => println!("file:   {h}"),
        Err(e) => println!("file:   cannot read header ({e})"),
    }
    let t = Instant::now();
    let db = match open(&bytes, &key) {
        Ok(db) => db,
        Err(e) => {
            println!("open:   FAILED — {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "open:   ok in {} — {} entries, {} groups, {} attachments",
        ms(t.elapsed()),
        db.entries().len(),
        db.groups().len(),
        db.binaries().len()
    );
    let saved = match save(&db, &key) {
        Ok(s) => s,
        Err(e) => {
            println!("save:   FAILED — {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Ok(h) = header::read(&saved) {
        println!("saved:  {h}");
    }
    let reopened = match open(&saved, &key) {
        Ok(db) => db,
        Err(e) => {
            println!("reopen: FAILED — {e}");
            return ExitCode::FAILURE;
        }
    };
    let tree_diffs = model::tree_diff(db.document(), reopened.document(), 50);
    let attachments_same = db.binaries() == reopened.binaries();
    println!(
        "\ntree before vs after (node by node): {}",
        if tree_diffs.is_empty() && attachments_same {
            "identical".to_string()
        } else {
            format!("{} differences", tree_diffs.len())
        }
    );
    for d in &tree_diffs {
        println!("  {d}");
    }
    if !attachments_same {
        println!("  attachment pool differs");
    }
    let (raw_a, raw_b) = match (raw_xml(&bytes, &key), raw_xml(&saved, &key)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            println!("cannot decrypt XML: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (ca, cb) = match (
        canon::parse(&raw_a, Source::Raw),
        canon::parse(&raw_b, Source::Raw),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            println!("cannot parse XML: {e}");
            return ExitCode::FAILURE;
        }
    };
    let raw_diffs = canon::diff(&ca, &cb);
    let sorted = sort_diffs(&raw_diffs, &ca);
    println!(
        "stored XML before vs after: {} records compared — {} unexpected, {} harmless",
        ca.records.len(),
        sorted.real.len(),
        sorted.benign.len()
    );
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for d in &sorted.real {
        let generic: String = canon::segments(&d.path)
            .into_iter()
            .map(|seg| {
                if seg.starts_with("Group[") || seg.starts_with("Entry[") {
                    seg.split('[').next().unwrap_or(seg).to_string() + "[*]"
                } else {
                    seg.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        let kind = match d.change {
            Change::Missing(_) => "LOST",
            Change::Added(_) => "ADDED",
            Change::Changed(_, _) => "CHANGED",
        };
        *by_kind.entry(format!("{kind:8} {generic}")).or_default() += 1;
    }
    for (k, n) in &by_kind {
        println!("  {n:5} × {k}");
    }
    if sorted.real.is_empty() && tree_diffs.is_empty() && attachments_same {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---------------------------------------------------------------- timing mode

fn run_timing() -> ExitCode {
    let fixtures = workspace_root().join("tests").join("fixtures");
    let key = Key {
        password: Some("mikoshi-test".into()),
        keyfile: None,
    };
    let Ok(bytes) = std::fs::read(fixtures.join("kxc/kdbx41-pw.kdbx")) else {
        eprintln!("run tests/fixtures/generate.py first");
        return ExitCode::FAILURE;
    };
    let Ok(mut base) = open(&bytes, &key) else {
        eprintln!("cannot open base fixture");
        return ExitCode::FAILURE;
    };
    let argon = |id, memory, iterations, parallelism| Kdf::Argon2 {
        id,
        memory,
        iterations,
        parallelism,
        version: 0x13,
    };
    // keepass-rs 0.17, measured on this machine in M0 (release build)
    let cases: Vec<(&str, Kdf, &str)> = vec![
        (
            "AES-KDF, 10,000,000 rounds",
            Kdf::Aes { rounds: 10_000_000 },
            "252 ms",
        ),
        (
            "Argon2d, 64 MiB, 2 iterations, 2 lanes (KeePass 2 default)",
            argon(false, 64 << 20, 2, 2),
            "96 ms",
        ),
        (
            "Argon2id, 64 MiB, 10 iterations, 4 lanes",
            argon(true, 64 << 20, 10, 4),
            "216 ms",
        ),
        (
            "Argon2id, 1 GiB, 2 iterations, 8 lanes",
            argon(true, 1 << 30, 2, 8),
            "1083 ms",
        ),
    ];
    let kxc = Kxc::find();
    let dir = out_dir().join("timing");
    let _ = std::fs::create_dir_all(&dir);
    // keepassxc-cli's own start-up cost, measured on a file with a trivial KDF
    let kxc_baseline = kxc.as_ref().and_then(|k| {
        let mut s = base.settings();
        s.kdf = Kdf::Aes { rounds: 1 };
        base.set_settings(&s);
        let path = dir.join("baseline.kdbx");
        std::fs::write(&path, save(&base, &key).ok()?).ok()?;
        (0..3)
            .filter_map(|_| {
                let t = Instant::now();
                k.db_info(&path, &key).ok().map(|_| t.elapsed())
            })
            .min()
    });
    println!(
        "build: {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!(
        "{:60} {:>10} {:>16} {:>16}",
        "KDF", "kdbx", "keepass-rs (M0)", "keepassxc-cli*"
    );
    for (i, (name, kdf, m0)) in cases.into_iter().enumerate() {
        let mut s = base.settings();
        s.kdf = kdf;
        base.set_settings(&s);
        let saved = match save(&base, &key) {
            Ok(s) => s,
            Err(e) => {
                println!("{name}: save failed: {e}");
                continue;
            }
        };
        let mut best = Duration::MAX;
        for _ in 0..3 {
            let t = Instant::now();
            if open(&saved, &key).is_err() {
                println!("{name}: reopen failed");
                break;
            }
            best = best.min(t.elapsed());
            if best > Duration::from_secs(3) {
                break;
            }
        }
        let theirs = match (&kxc, kxc_baseline) {
            (Some(k), Some(base_t)) => {
                let path = dir.join(format!("kdf-{i}.kdbx"));
                let _ = std::fs::write(&path, &saved);
                (0..2)
                    .filter_map(|_| {
                        let t = Instant::now();
                        k.db_info(&path, &key).ok().map(|_| t.elapsed())
                    })
                    .min()
                    .map(|t| format!("{} ms", t.saturating_sub(base_t).as_millis()))
                    .unwrap_or_else(|| "failed".into())
            }
            _ => "-".into(),
        };
        println!(
            "{name:60} {:>7} ms {:>16} {:>16}",
            best.as_millis(),
            m0,
            theirs
        );
    }
    if let Some(b) = kxc_baseline {
        println!(
            "* keepassxc-cli db-info time minus its start-up time ({} ms)",
            b.as_millis()
        );
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("fixtures") => run_fixtures(),
        Some("check") => run_check(&args[1..]),
        Some("timing") => run_timing(),
        Some("fuzz") => run_fuzz(&args[1..]),
        _ => {
            eprintln!(
                "usage: kdbx-compat [fixtures | check <file> [--keyfile <path>] [--no-password] | timing | fuzz [seconds] [seed]]"
            );
            ExitCode::FAILURE
        }
    }
}
