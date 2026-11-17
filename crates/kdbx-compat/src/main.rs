//! kdbx-compat: checks that keepass-rs reads KeePass files exactly like
//! KeePassXC does, and that saving them loses nothing.
//!
//!   kdbx-compat [fixtures]          run every check on tests/fixtures, write the report
//!   kdbx-compat check <file> [--keyfile <path>] [--no-password | --password-stdin]
//!                                   round-trip one file in memory and list what would
//!                                   change; prints paths only, never values, and never
//!                                   writes anything
//!   kdbx-compat timing              time opening files with typical KDF settings

mod canon;
mod errors;
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

use keepass::Database;
use keepass::config::{
    DatabaseConfig, DatabaseVersion, InnerCipherConfig, KdfConfig, OuterCipherConfig,
};
use keepass::db::{GroupId, Value, fields};
use serde::Deserialize;

use canon::{Canon, Change, Diff, Source};
use errors::Failure;
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
    Failed(Failure, String),
    Panicked(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Failed(kind, msg) => write!(f, "{} ({msg})", kind.label()),
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

/// Opens a database, turning panics inside keepass-rs into errors.
fn open(bytes: &[u8], key: &Key) -> Result<Database, OpenError> {
    let dk = key
        .to_keepass()
        .map_err(|e| OpenError::Failed(Failure::Io, e.to_string()))?;
    // keep expected panics out of the console; they are reported instead
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(|| Database::parse(bytes, dk)));
    std::panic::set_hook(previous);
    match result {
        Ok(Ok(db)) => Ok(db),
        Ok(Err(e)) => Err(OpenError::Failed(errors::classify(&e), e.to_string())),
        Err(p) => Err(OpenError::Panicked(panic_message(p))),
    }
}

fn raw_xml(bytes: &[u8], key: &Key) -> Result<String, String> {
    let dk = key.to_keepass().map_err(|e| e.to_string())?;
    let xml = Database::get_xml(&mut &bytes[..], dk).map_err(|e| e.to_string())?;
    String::from_utf8(xml).map_err(|e| e.to_string())
}

/// What the app will have to do before saving: keepass-rs only writes
/// KDBX 4.1, so 3.1 and 4.0 files are upgraded. KDBX 4 normally uses ChaCha20
/// for protected values (KeePass does the same when it upgrades a 3.1 file).
fn prepare_for_save(db: &Database) -> Database {
    let mut db = db.clone();
    db.config.version = DatabaseVersion::KDB4(1);
    if matches!(db.config.inner_cipher_config, InnerCipherConfig::Salsa20) {
        db.config.inner_cipher_config = InnerCipherConfig::ChaCha20;
    }
    db
}

fn save(db: &Database, key: &Key) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let dk = key.to_keepass().map_err(|e| e.to_string())?;
    match catch_unwind(AssertUnwindSafe(|| db.save(&mut out, dk))) {
        Ok(Ok(())) => Ok(out),
        Ok(Err(e)) => Err(e.to_string()),
        Err(p) => Err(format!("PANIC: {}", panic_message(p))),
    }
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

// ---------------------------------------------------------------- diff classification

/// Problems found while evaluating keepass-rs for M0. A run that only hits
/// these is reported as "known issues", not as a failure, so the tool keeps
/// working as a regression check once the issues are fixed or worked around.
pub const KNOWN: &[(&str, &str)] = &[
    (
        "K1",
        "An entry or group with a custom icon loses its standard icon number (IconID), which other apps fall back to",
    ),
    (
        "K2",
        "A field whose value is only whitespace is read as empty (and saved empty)",
    ),
    (
        "K3",
        "TOTP: an otpauth URI without `digits` gives 8 digits; the standard and KeePassXC use 6",
    ),
    ("K4", "TOTP: a lower-case Base32 secret cannot be parsed"),
    (
        "K5",
        "KDBX 3.1: a wrong key and a damaged file cannot be told apart (a wrong password usually surfaces as a padding error)",
    ),
    (
        "K6",
        "XML elements keepass-rs does not know are dropped on save (KeePassXC drops them too)",
    ),
    (
        "K7",
        "Meta/MasterKeyChangeForceOnce (KeePass 2: \"force changing the master key next time\") is dropped on save",
    ),
    (
        "K8",
        "An unknown element directly inside a Group makes the whole file fail to open",
    ),
    (
        "K9",
        "A malformed timestamp makes keepass-rs panic instead of returning an error",
    ),
];

enum Verdict {
    Benign(&'static str),
    Known(&'static str),
    Real,
}

fn whitespace_only(v: &str) -> bool {
    !v.is_empty() && v.trim().is_empty()
}

/// `before` is the document the diff started from, for context.
fn classify_diff(d: &Diff, before: &Canon) -> Verdict {
    let p = d.path.as_str();
    match &d.change {
        Change::Added(v) if p.ends_with("/QualityCheck") && v == "True" => {
            Verdict::Benign("QualityCheck=True written explicitly (absent means True)")
        }
        Change::Added(v) if p.ends_with("/AutoType/DataTransferObfuscation") && v == "0" => {
            Verdict::Benign("default value written explicitly")
        }
        Change::Added(_) if p == "Meta/SettingsChanged" => {
            Verdict::Benign("KeePassXC shows a SettingsChanged time once the file is KDBX 4")
        }
        Change::Missing(_)
            if p.ends_with("/IconID")
                && before.records.contains_key(&format!(
                    "{}/CustomIconUUID",
                    &p[..p.len() - "/IconID".len()]
                )) =>
        {
            Verdict::Known("K1")
        }
        Change::Missing(v) | Change::Changed(v, _)
            if p.ends_with("/Value") && whitespace_only(v) =>
        {
            Verdict::Known("K2")
        }
        Change::Missing(_)
            if canon::segments(p)
                .iter()
                .any(|s| s.starts_with("MikoshiUnknown")) =>
        {
            Verdict::Known("K6")
        }
        Change::Missing(_) if p == "Meta/MasterKeyChangeForceOnce" => Verdict::Known("K7"),
        _ => Verdict::Real,
    }
}

struct Sorted<'a> {
    real: Vec<&'a Diff>,
    known: Vec<(&'a Diff, &'static str)>,
    benign: Vec<(&'a Diff, &'static str)>,
}

fn sort_diffs<'a>(diffs: &'a [Diff], before: &Canon) -> Sorted<'a> {
    let mut s = Sorted {
        real: Vec::new(),
        known: Vec::new(),
        benign: Vec::new(),
    };
    for d in diffs {
        match classify_diff(d, before) {
            Verdict::Benign(why) => s.benign.push((d, why)),
            Verdict::Known(id) => s.known.push((d, id)),
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
    for (d, id) in &sorted.known {
        sec.known(id, format!("{prefix} {}", describe(d, labels, show_values)));
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
    known: Vec<&'static str>,
    lines: Vec<String>,
}

impl Section {
    fn new(title: impl Into<String>) -> Self {
        Section {
            title: title.into(),
            ok: true,
            known: Vec::new(),
            lines: Vec::new(),
        }
    }
    fn fail(&mut self, line: impl Into<String>) {
        self.ok = false;
        self.lines.push(line.into());
    }
    fn known(&mut self, id: &'static str, line: impl Into<String>) {
        self.mark_known(id);
        self.lines.push(format!("[{id}] {}", line.into()));
    }
    fn mark_known(&mut self, id: &'static str) {
        if !self.known.contains(&id) {
            self.known.push(id);
        }
    }
    fn note(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }
    fn status(&self) -> String {
        if !self.ok {
            "PROBLEMS".into()
        } else if !self.known.is_empty() {
            format!("ok, known issues {}", self.known.join(" "))
        } else {
            "ok".into()
        }
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
}

fn run_fixtures() -> ExitCode {
    let fixtures = workspace_root().join("tests").join("fixtures");
    let manifest_text = match std::fs::read_to_string(fixtures.join("fixtures.toml")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read tests/fixtures/fixtures.toml: {e}");
            return ExitCode::FAILURE;
        }
    };
    let manifest: Manifest = match toml::from_str(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("bad fixtures.toml: {e}");
            return ExitCode::FAILURE;
        }
    };
    let kxc = Kxc::find();
    let ctx = Ctx {
        fixtures,
        manifest,
        kxc,
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

    eprintln!("\ncipher / KDF matrix written by keepass-rs ...");
    let matrix = run_matrix(&ctx);
    eprintln!("elements KeePassXC does not keep ...");
    let foreign = run_foreign(&ctx);
    eprintln!("damaged files ...");
    let damaged = run_damaged(&ctx);
    eprintln!("editing ...");
    let editing = run_edit(&ctx);

    let extra = [&matrix, &foreign, &damaged, &editing];
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
    let labels_holder;

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
            sec.fail(format!("keepass-rs cannot open it: {e}"));
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
        db.iter_all_entries().count(),
        db.iter_all_groups().count()
    ));
    report.sections.push(sec);

    let ours = model::from_database(&db);

    // ---- read vs KeePassXC
    let mut theirs: Option<Canon> = None;
    if let Some(kxc) = &ctx.kxc {
        let mut sec = Section::new("Read: keepass-rs vs KeePassXC");
        match kxc
            .export_xml(&path, &key)
            .and_then(|x| canon::parse(&x, Source::Export))
        {
            Ok(c) => {
                // KeePassXC stamps _LAST_MODIFIED with the export time
                let mut ours_cmp = ours.clone();
                ours_cmp
                    .records
                    .retain(|k, _| !k.ends_with("Item[_LAST_MODIFIED]/Value"));
                let diffs = canon::diff(&c, &ours_cmp);
                let labels = [&c.labels, &ours.labels];
                let sorted = sort_diffs(&diffs, &c);
                if sorted.real.is_empty() && sorted.known.is_empty() {
                    sec.note(format!("identical ({} records compared)", c.records.len()));
                }
                let mut sub = Section::new("");
                report_diffs(&mut sub, &sorted, &labels, "", true);
                for l in sub.lines {
                    let l = l
                        .replace("- LOST    ", "- ONLY IN KEEPASSXC")
                        .replace("- ADDED   ", "- ONLY IN KEEPASS-RS");
                    sec.lines.push(l);
                }
                sec.ok &= sub.ok;
                for k in sub.known {
                    if !sec.known.contains(&k) {
                        sec.known.push(k);
                    }
                }
                theirs = Some(c);
            }
            Err(e) => sec.fail(format!("keepassxc-cli export failed: {e}")),
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
        Err(OpenError::Failed(Failure::WrongKey, _)) => sec.note("reported as wrong key"),
        Err(OpenError::Failed(Failure::Ambiguous, msg)) if spec.format == "3.1" => {
            sec.known("K5", format!("reported as \"{msg}\""))
        }
        Err(e) => sec.fail(format!("wrong password reported as: {e}")),
    }
    report.sections.push(sec);

    // ---- round trip
    let mut sec = Section::new("Round trip: open → save → reopen");
    let prepared = prepare_for_save(&db);
    let t = Instant::now();
    let saved = match save(&prepared, &key) {
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
        Ok(h) => sec.note(format!("saved in {} as {h}", ms(save_time))),
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

    // keepass-rs' own view before and after
    let after = model::from_database(&reopened);
    let model_diffs = canon::diff(&ours, &after);
    let labels = [&ours.labels];
    if model_diffs.is_empty() {
        sec.note("keepass-rs model before vs after: identical");
    } else {
        report_diffs(
            &mut sec,
            &sort_diffs(&model_diffs, &ours),
            &labels,
            "keepass-rs model",
            true,
        );
    }
    if model::attachments(&db) == model::attachments(&reopened) {
        sec.note(format!(
            "attachments (history included): {} identical",
            model::attachments(&db).len()
        ));
    } else {
        sec.fail("attachment contents changed");
    }

    // the XML actually stored in the file, before and after
    match (raw_xml(&bytes, &key), raw_xml(&saved, &key)) {
        (Ok(a), Ok(b)) => match (canon::parse(&a, Source::Raw), canon::parse(&b, Source::Raw)) {
            (Ok(ca), Ok(cb)) => {
                let diffs = canon::diff(&ca, &cb);
                labels_holder = ca.labels.clone();
                let labels = [&labels_holder];
                let sorted = sort_diffs(&diffs, &ca);
                if sorted.real.is_empty() && sorted.known.is_empty() {
                    sec.note(format!(
                        "stored XML: nothing lost ({} records compared)",
                        ca.records.len()
                    ));
                } else {
                    sec.note(format!("stored XML: {} records compared", ca.records.len()));
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
                if sorted.real.is_empty() && sorted.known.is_empty() {
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
    let mut known: Vec<&str> = report
        .sections
        .iter()
        .flat_map(|s| s.known.iter().copied())
        .collect();
    known.sort();
    known.dedup();
    report.status = if failures.is_empty() { "ok" } else { "FAIL" };
    report.summary = if failures.is_empty() {
        format!(
            "opened in {}{}",
            ms(open_time),
            if known.is_empty() {
                String::new()
            } else {
                format!("; known issues {}", known.join(" "))
            }
        )
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
    let entries = model::cli_entry_paths(db);
    let mut checked = 0;
    for e in db.iter_all_entries() {
        if e.get(fields::OTP).is_none() {
            continue;
        }
        let Some((_, cli)) = entries
            .iter()
            .find(|(p, _)| p.ends_with(&format!("Entry[{}]", base64_uuid(e.id().uuid()))))
        else {
            continue;
        };
        let raw = e.get(fields::OTP).unwrap_or("");
        let ours = match e.get_otp() {
            Ok(t) => t,
            Err(err) => {
                let secret = raw
                    .split("secret=")
                    .nth(1)
                    .and_then(|s| s.split('&').next())
                    .unwrap_or("");
                let line = format!("TOTP of \"{cli}\": keepass-rs cannot parse it ({err})");
                if secret.chars().any(|c| c.is_ascii_lowercase()) {
                    sec.known("K4", line)
                } else {
                    sec.fail(line)
                }
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
            .map(|&t| ours.value_at(t).code)
            .collect();
        mine.dedup();
        checked += 1;
        if !mine.contains(&theirs_code) {
            let line = format!(
                "TOTP of \"{cli}\" differs: keepass-rs {}, KeePassXC {theirs_code}",
                mine.join(" or ")
            );
            if !raw.contains("digits=")
                && theirs_code.len() == 6
                && mine
                    .iter()
                    .any(|m| m.len() == 8 && m.ends_with(&theirs_code))
            {
                sec.known("K3", line);
            } else {
                sec.fail(line);
            }
        }
    }
    if checked > 0 {
        sec.note(format!("TOTP: {checked} codes compared with KeePassXC"));
    }
}

fn base64_uuid(u: impl AsRef<[u8]>) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(u.as_ref())
}

// ---------------------------------------------------------------- cipher / KDF matrix

fn fast_kdfs() -> Vec<(&'static str, KdfConfig)> {
    let version = match DatabaseConfig::default().kdf_config {
        KdfConfig::Argon2 { version, .. } => version,
        _ => unreachable_version(),
    };
    vec![
        ("AES-KDF", KdfConfig::Aes { rounds: 100_000 }),
        (
            "Argon2d",
            KdfConfig::Argon2 {
                iterations: 2,
                memory: 16 * 1024 * 1024,
                parallelism: 2,
                version,
            },
        ),
        (
            "Argon2id",
            KdfConfig::Argon2id {
                iterations: 2,
                memory: 16 * 1024 * 1024,
                parallelism: 2,
                version,
            },
        ),
    ]
}

fn unreachable_version() -> ! {
    panic!("keepass-rs default config no longer uses Argon2")
}

fn run_matrix(ctx: &Ctx) -> Section {
    let mut sec = Section::new("Cipher / KDF matrix written by keepass-rs");
    let Some(spec) = ctx
        .manifest
        .fixture
        .iter()
        .find(|f| f.file == "kxc/kdbx41-pw.kdbx")
        .cloned()
    else {
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
    let base_model = model::from_database(&db);
    let base_export = ctx
        .kxc
        .as_ref()
        .and_then(|k| k.export_xml(&base_path, &key).ok())
        .and_then(|x| canon::parse(&x, Source::Export).ok());

    sec.note(
        "| cipher | KDF | keepass-rs reopens | KeePassXC opens | KeePassXC sees same data |"
            .to_string(),
    );
    sec.note("|---|---|---|---|---|".to_string());
    for (cipher_name, cipher) in [
        ("AES-256", OuterCipherConfig::AES256),
        ("Twofish", OuterCipherConfig::Twofish),
        ("ChaCha20", OuterCipherConfig::ChaCha20),
    ] {
        for (kdf_name, kdf) in fast_kdfs() {
            let mut variant = prepare_for_save(&db);
            variant.config.outer_cipher_config = cipher.clone();
            variant.config.kdf_config = kdf;
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
                Ok(r) if canon::diff(&base_model, &model::from_database(&r)).is_empty() => {
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
                        let mut known: Vec<&str> = sorted.known.iter().map(|(_, id)| *id).collect();
                        known.sort();
                        known.dedup();
                        let verdict = if !sorted.real.is_empty() {
                            format!("{} differences", sorted.real.len())
                        } else if !known.is_empty() {
                            format!("yes, except known {}", known.join(" "))
                        } else {
                            "yes".to_string()
                        };
                        ("yes".to_string(), verdict)
                    }
                    Err(e) => (format!("no: {e}"), "-".to_string()),
                },
                _ => ("(no keepassxc-cli)".to_string(), "-".to_string()),
            };
            let row = format!("| {cipher_name} | {kdf_name} | {reopen} | {opens} | {same} |");
            if reopen == "yes" && opens.starts_with("yes") && same.starts_with("yes") {
                if let Some(rest) = same.strip_prefix("yes, except known ") {
                    for id in rest.split(' ') {
                        if let Some(id) = KNOWN.iter().find(|(k, _)| *k == id).map(|(k, _)| *k)
                            && !sec.known.contains(&id)
                        {
                            sec.known.push(id);
                        }
                    }
                }
                sec.note(row);
            } else {
                sec.fail(row);
            }
        }
    }
    sec
}

// ---------------------------------------------------------------- elements KeePassXC does not keep

/// KeePassXC drops unknown elements when it imports XML, so these files are
/// written by our own minimal writer (src/writer.rs) from the fixture content.
fn run_foreign(ctx: &Ctx) -> Section {
    use writer::{Injection, Target};
    let mut sec = Section::new("Elements KeePassXC does not keep (files written by src/writer.rs)");
    let Ok(content) = std::fs::read_to_string(ctx.fixtures.join("src/content-41.xml")) else {
        sec.fail("tests/fixtures/src/content-41.xml missing; run tests/fixtures/generate.py");
        return sec;
    };
    let password = ctx.manifest.password.clone();
    let key = Key {
        password: Some(password.clone()),
        keyfile: None,
    };
    let dir = out_dir().join("foreign");
    let _ = std::fs::create_dir_all(&dir);

    let cases: Vec<(&str, String, Vec<Injection>)> = vec![
        (
            "unknown elements in Meta, Root, Entry, AutoType + KeePass 2's MasterKeyChangeForceOnce",
            content.clone(),
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
            "unknown element directly inside a Group",
            content.clone(),
            vec![Injection {
                target: Target::Group("General"),
                xml: "<MikoshiUnknownGroup>group-level unknown element</MikoshiUnknownGroup>",
            }],
        ),
        (
            "malformed (too short) timestamp",
            content.replacen(
                "<LastAccessTime>2024-01-10T08:00:00Z</LastAccessTime>",
                "<LastAccessTime>AAAA</LastAccessTime>",
                1,
            ),
            vec![],
        ),
    ];

    for (i, (what, xml, injections)) in cases.into_iter().enumerate() {
        let file = dir.join(format!("foreign-{}.kdbx", (b'a' + i as u8) as char));
        let bytes = match writer::write(&xml, &password, &injections) {
            Ok(b) => b,
            Err(e) => {
                sec.fail(format!("{what}: cannot write test file: {e}"));
                continue;
            }
        };
        let _ = std::fs::write(&file, &bytes);
        let kxc_view = ctx.kxc.as_ref().map(|k| k.export_xml(&file, &key));
        match &kxc_view {
            Some(Ok(_)) if injections.is_empty() => sec.note(format!("{what}: KeePassXC opens it")),
            Some(Ok(x)) => {
                let kept: Vec<&str> = ["MikoshiUnknown", "MasterKeyChangeForceOnce"]
                    .into_iter()
                    .filter(|n| x.contains(n))
                    .collect();
                sec.note(format!(
                    "{what}: KeePassXC opens it{}",
                    if kept.is_empty() {
                        " and drops the extra elements".to_string()
                    } else {
                        format!(", keeps {}", kept.join(", "))
                    }
                ));
            }
            Some(Err(e)) if what.starts_with("malformed") => {
                sec.note(format!("{what}: KeePassXC refuses it too ({e})"))
            }
            Some(Err(e)) => {
                sec.fail(format!(
                    "{what}: KeePassXC cannot open the test file, so the writer is wrong: {e}"
                ));
                continue;
            }
            None => {}
        }

        let db = match open(&bytes, &key) {
            Ok(db) => db,
            Err(OpenError::Panicked(msg)) if what.starts_with("malformed") => {
                sec.known("K9", format!("{what}: keepass-rs panics ({msg})"));
                continue;
            }
            Err(e) if what.starts_with("malformed") => {
                sec.note(format!("{what}: keepass-rs refuses it with an error: {e}"));
                continue;
            }
            Err(e) if what.contains("inside a Group") => {
                sec.known(
                    "K8",
                    format!("{what}: keepass-rs cannot open the file: {e}"),
                );
                continue;
            }
            Err(e) => {
                sec.fail(format!("{what}: keepass-rs cannot open it: {e}"));
                continue;
            }
        };
        let saved = match save(&prepare_for_save(&db), &key) {
            Ok(s) => s,
            Err(e) => {
                sec.fail(format!("{what}: save failed: {e}"));
                continue;
            }
        };
        match (raw_xml(&bytes, &key), raw_xml(&saved, &key)) {
            (Ok(a), Ok(b)) => {
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
    }
    sec
}

// ---------------------------------------------------------------- damaged files

fn run_damaged(ctx: &Ctx) -> Section {
    let mut sec = Section::new("Damaged files and wrong keys");
    sec.note("| file | damage | reported as |".to_string());
    sec.note("|---|---|---|".to_string());
    for name in ["kxc/kdbx41-pw.kdbx", "kxc/kdbx31-pw.kdbx"] {
        let Some(spec) = ctx
            .manifest
            .fixture
            .iter()
            .find(|f| f.file == name)
            .cloned()
        else {
            continue;
        };
        let key = ctx.key(&spec);
        let Ok(bytes) = std::fs::read(ctx.fixtures.join(name)) else {
            continue;
        };
        let header_len = header_length(&bytes).unwrap_or(200);
        let mut cases: Vec<(&str, Vec<u8>, Key)> = Vec::new();
        cases.push((
            "wrong password",
            bytes.clone(),
            Key {
                password: Some("not-the-password".into()),
                keyfile: None,
            },
        ));
        cases.push((
            "no password, key file only",
            bytes.clone(),
            Key {
                password: None,
                keyfile: Some(ctx.fixtures.join("keys/binary-32.key")),
            },
        ));
        let mut t = bytes.clone();
        t.truncate(bytes.len() / 2);
        cases.push(("truncated to half", t, key.clone()));
        let mut t = bytes.clone();
        t.truncate(header_len.saturating_sub(10));
        cases.push(("truncated inside the header", t, key.clone()));
        let mut t = bytes.clone();
        if let Some(b) = t.get_mut(header_len.saturating_sub(20)) {
            *b ^= 0x01;
        }
        cases.push(("one bit flipped in the header", t, key.clone()));
        let mut t = bytes.clone();
        if let Some(b) = t.get_mut(header_len + 80) {
            *b ^= 0x01;
        }
        cases.push((
            "one bit flipped at the start of the payload",
            t,
            key.clone(),
        ));
        let mut t = bytes.clone();
        let n = t.len();
        if let Some(b) = t.get_mut(n - 40) {
            *b ^= 0x01;
        }
        cases.push(("one bit flipped near the end", t, key.clone()));
        for (what, data, k) in cases {
            let result = match open(&data, &k) {
                Ok(damaged_db) => {
                    // only acceptable if the damage did not touch the content
                    let same = open(&bytes, &key)
                        .map(|orig| {
                            canon::diff(
                                &model::from_database(&orig),
                                &model::from_database(&damaged_db),
                            )
                            .is_empty()
                        })
                        .unwrap_or(false);
                    if same {
                        sec.note(format!("| {name} | {what} | opened; content identical to the original (the flipped bit is in the end-of-stream marker, which is not verified) |"));
                    } else {
                        sec.fail(format!(
                            "| {name} | {what} | OPENED WITH DIFFERENT CONTENT |"
                        ));
                    }
                    continue;
                }
                Err(e) => e.to_string(),
            };
            if result.contains("PANIC") {
                sec.fail(format!("| {name} | {what} | {result} |"));
                continue;
            }
            let expected_wrong_key = what.starts_with("wrong") || what.starts_with("no password");
            let says_wrong_key = result.starts_with("wrong key");
            if spec.format == "3.1"
                && (result.starts_with("AMBIGUOUS") || expected_wrong_key != says_wrong_key)
            {
                sec.mark_known("K5");
                sec.note(format!("| {name} | {what} | {result} [K5] |"));
            } else if expected_wrong_key != says_wrong_key {
                sec.fail(format!("| {name} | {what} | {result} |"));
            } else {
                sec.note(format!("| {name} | {what} | {result} |"));
            }
        }
    }
    let random: Vec<u8> = (0..4096u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    for (what, data) in [
        ("empty file", Vec::new()),
        ("random bytes", random),
        ("KeePass 1 (.kdb) signature", kdb_stub()),
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
        let line = format!("| - | {what} | {result} |");
        if result.contains("PANIC") {
            sec.fail(line)
        } else {
            sec.note(line)
        }
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
    let mut sec = Section::new("Edit with keepass-rs, read back with KeePassXC");
    let Some(spec) = ctx
        .manifest
        .fixture
        .iter()
        .find(|f| f.file == "kxc/kdbx41-pw.kdbx")
        .cloned()
    else {
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
        db.iter_all_entries()
            .find(|e| e.get_title() == Some(title))
            .map(|e| e.id())
    };
    let find_group = |db: &Database, name: &str| {
        db.iter_all_groups()
            .find(|g| g.name == name)
            .map(|g| g.id())
    };
    let (Some(full), Some(expired), Some(general)) = (
        find(&db, "Full Fields"),
        find(&db, "Expired"),
        find_group(&db, "General"),
    ) else {
        sec.fail("fixture content not as expected");
        return sec;
    };
    let recycle: Option<GroupId> = db.recycle_bin().map(|g| g.id());

    // 1. edit an entry, keeping the old version in history (what Mikoshi will do)
    let history_before = db
        .entry(full)
        .and_then(|e| e.history.as_ref().map(|h| h.get_entries().len()))
        .unwrap_or(0);
    if let Some(mut e) = db.entry_mut(full) {
        let snapshot = (*e).clone();
        e.history
            .get_or_insert_with(Default::default)
            .add_entry(snapshot);
        e.set_unprotected(fields::TITLE, "Full Fields (edited)");
        e.set_protected(fields::PASSWORD, "new-password-✓");
        e.set_protected("New Secret", "added by keepass-rs");
    }
    // 2. new entry with an attachment, in a new group
    let new_group = db.group_mut(general).map(|mut g| {
        let mut ng = g.add_group();
        ng.name = "Added Group".into();
        ng.id()
    });
    let new_entry = new_group.and_then(|gid| {
        db.group_mut(gid).map(|mut g| {
            let mut e = g.add_entry();
            e.set_unprotected(fields::TITLE, "Added Entry");
            e.set_unprotected(fields::USERNAME, "added-user");
            e.set_protected(fields::PASSWORD, "added-password");
            e.add_attachment(
                "added.txt",
                Value::unprotected(b"attachment added by keepass-rs".to_vec()),
            );
            e.id()
        })
    });
    // 3. move an entry into the recycle bin
    if let (Some(bin), Some(mut e)) = (recycle, db.entry_mut(expired)) {
        let _ = e.move_to(bin);
    }

    let saved = match save(&prepare_for_save(&db), &key) {
        Ok(s) => s,
        Err(e) => {
            sec.fail(format!("save failed: {e}"));
            return sec;
        }
    };
    let path = out_dir().join("saved").join("edited.kdbx");
    let _ = std::fs::write(&path, &saved);

    let Some(kxc) = &ctx.kxc else {
        sec.note("keepassxc-cli not found; only checked that keepass-rs reopens it");
        if open(&saved, &key).is_err() {
            sec.fail("keepass-rs cannot reopen its own edited file");
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
    let full_key = format!("Entry[{}]", base64_uuid(full.uuid()));
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
        let k = format!("Entry[{}]", base64_uuid(id.uuid()));
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
                d == b"attachment added by keepass-rs",
            ),
            Err(e) => expect(&format!("attachment of the new entry ({e})"), false),
        }
    }
    if let Some(bin) = recycle {
        let exp = format!(
            "Group[{}]/Entry[{}]",
            base64_uuid(bin.uuid()),
            base64_uuid(expired.uuid())
        );
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
            prev == Some(base64_uuid(general.uuid())),
        );
    }
    sec
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
    let _ = writeln!(s, "- keepass-rs: 0.17 (features save_kdbx4, totp)");
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

    let _ = writeln!(s, "\n## Known issues\n");
    let _ = writeln!(
        s,
        "Found while evaluating keepass-rs; see docs/m0-report.md. Lines tagged with these ids are expected until fixed.\n"
    );
    for (id, text) in KNOWN {
        let _ = writeln!(s, "- **{id}** {text}");
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
        "open:   ok in {} — {} entries, {} groups",
        ms(t.elapsed()),
        db.iter_all_entries().count(),
        db.iter_all_groups().count()
    );
    let saved = match save(&prepare_for_save(&db), &key) {
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
    let before = model::from_database(&db);
    let model_diffs = canon::diff(&before, &model::from_database(&reopened));
    let attachments_same = model::attachments(&db) == model::attachments(&reopened);
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
        "\nkeepass-rs model before vs after: {}",
        if model_diffs.is_empty() && attachments_same {
            "identical".to_string()
        } else {
            format!("{} differences", model_diffs.len())
        }
    );
    for d in &model_diffs {
        println!("  {}", describe(d, &[&before.labels], false));
    }
    println!(
        "stored XML before vs after: {} records compared — {} unexpected, {} known issues, {} harmless",
        ca.records.len(),
        sorted.real.len(),
        sorted.known.len(),
        sorted.benign.len()
    );
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let tagged = sorted
        .real
        .iter()
        .map(|d| (*d, "NEW"))
        .chain(sorted.known.iter().map(|(d, id)| (*d, *id)));
    for (d, tag) in tagged {
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
        *by_kind
            .entry(format!("[{tag}] {kind:8} {generic}"))
            .or_default() += 1;
    }
    for (k, n) in &by_kind {
        println!("  {n:5} × {k}");
    }
    let ids: Vec<&str> = sorted.known.iter().map(|(_, id)| *id).collect();
    for (id, text) in KNOWN.iter().filter(|(id, _)| ids.contains(id)) {
        println!("  {id}: {text}");
    }
    if sorted.real.is_empty() && model_diffs.is_empty() && attachments_same {
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
    let Ok(base) = open(&bytes, &key) else {
        eprintln!("cannot open base fixture");
        return ExitCode::FAILURE;
    };
    let version = match DatabaseConfig::default().kdf_config {
        KdfConfig::Argon2 { version, .. } => version,
        _ => unreachable_version(),
    };
    let cases: Vec<(&str, KdfConfig)> = vec![
        (
            "AES-KDF, 10,000,000 rounds",
            KdfConfig::Aes { rounds: 10_000_000 },
        ),
        (
            "Argon2d, 64 MiB, 2 iterations, 2 lanes (KeePass 2 default)",
            KdfConfig::Argon2 {
                iterations: 2,
                memory: 64 << 20,
                parallelism: 2,
                version,
            },
        ),
        (
            "Argon2id, 64 MiB, 10 iterations, 4 lanes",
            KdfConfig::Argon2id {
                iterations: 10,
                memory: 64 << 20,
                parallelism: 4,
                version,
            },
        ),
        (
            "Argon2id, 1 GiB, 2 iterations, 8 lanes",
            KdfConfig::Argon2id {
                iterations: 2,
                memory: 1 << 30,
                parallelism: 8,
                version,
            },
        ),
    ];
    let kxc = Kxc::find();
    let dir = out_dir().join("timing");
    let _ = std::fs::create_dir_all(&dir);
    // keepassxc-cli's own start-up cost, measured on a file with a trivial KDF
    let kxc_baseline = kxc.as_ref().and_then(|k| {
        let mut db = prepare_for_save(&base);
        db.config.kdf_config = KdfConfig::Aes { rounds: 1 };
        let path = dir.join("baseline.kdbx");
        std::fs::write(&path, save(&db, &key).ok()?).ok()?;
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
    println!("{:60} {:>12} {:>16}", "KDF", "keepass-rs", "keepassxc-cli*");
    for (i, (name, kdf)) in cases.into_iter().enumerate() {
        let mut db = prepare_for_save(&base);
        db.config.kdf_config = kdf;
        let saved = match save(&db, &key) {
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
        println!("{name:60} {:>9} ms {:>16}", best.as_millis(), theirs);
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
        _ => {
            eprintln!(
                "usage: kdbx-compat [fixtures | check <file> [--keyfile <path>] [--no-password] | timing]"
            );
            ExitCode::FAILURE
        }
    }
}
