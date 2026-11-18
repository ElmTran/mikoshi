//! An unlocked KDBX database and the views of it the frontend may see.
//!
//! Opening never writes anything. The decrypted database lives in [`Vault`];
//! dropping it (on lock) releases it all. keepass-rs keeps protected values in
//! `SecretBox`, which wipes them on drop; unprotected fields (user names,
//! URLs, notes) are ordinary strings inside the library (see m0-report.md).

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::NaiveDateTime;
use keepass::config::DatabaseVersion;
use keepass::db::{EntryId, EntryRef, GroupId, GroupRef, Icon, Times};
use keepass::error::{
    DatabaseFormatError, DatabaseOpenError, DatabaseVersionParseError as V, Kdbx3OpenError,
    Kdbx3OuterHeaderError as H3, Kdbx4OpenError, Kdbx4OuterHeaderError as H4,
};
use keepass::{Database, DatabaseKey};

use crate::Secret;
use crate::autotype::{self, DEFAULT_SEQUENCE};
use crate::model::{
    AttachmentView, CustomIconView, EntryDetail, EntrySummary, FieldView, GroupKind, GroupView,
    TotpView, fields,
};
use crate::search::{Haystack, Query};
use crate::totp::{self, Totp, TotpError};

/// Why a database could not be opened. Carries no user data, so it can be
/// logged and shown as is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// neither a password nor a key file was given
    NoKey,
    /// the password or the key file is wrong
    WrongKey,
    /// KDBX 3.1 cannot tell a wrong key from a damaged file (M0 issue K5)
    WrongKeyOrCorrupt,
    /// the file is damaged or truncated
    Corrupt,
    /// a KeePass file, but a version or algorithm that is not supported
    Unsupported,
    /// a KeePass 1 `.kdb` file: keepass-rs reads them, Mikoshi does not
    KeePass1,
    /// not a KeePass file at all
    NotKeePass,
    FileNotFound,
    FileUnreadable,
    KeyFileUnreadable,
}

/// What unlocks a database. An empty password counts as "no password".
pub struct Credentials<'a> {
    pub password: Option<&'a Secret>,
    pub key_file: Option<&'a Path>,
}

/// Standard KeePass icon numbers used as defaults.
const ICON_KEY: u32 = 0;
const ICON_FOLDER: u32 = 48;
const ICON_COUNT: usize = 69;

/// Fields that hold TOTP settings or secrets. They are never listed as
/// custom fields or revealed: the code is computed on the Rust side
/// (features.md §6).
fn is_otp_field(key: &str) -> bool {
    totp::is_totp_field(key)
}

fn has_totp(e: &EntryRef<'_>) -> bool {
    totp::has_totp(|k| e.get(k))
}

fn is_standard_field(key: &str) -> bool {
    matches!(
        key,
        fields::TITLE | fields::USERNAME | fields::PASSWORD | fields::URL | fields::NOTES
    )
}

fn iso(t: &NaiveDateTime) -> String {
    format!("{}Z", t.format("%Y-%m-%dT%H:%M:%S"))
}

fn expires(times: &Times) -> Option<&NaiveDateTime> {
    (times.expires == Some(true)).then_some(&times.expiry)
}

fn expired(times: &Times) -> bool {
    expires(times).is_some_and(|t| *t <= Times::now())
}

/// Standard icon number and custom icon id. `default` applies when the file
/// gives no icon or one outside the standard range.
fn icon_of(icon: Option<&Icon>, default: u32) -> (u32, Option<String>) {
    match icon {
        Some(Icon::BuiltIn(i)) if *i < ICON_COUNT => (*i as u32, None),
        Some(Icon::Custom(id)) => (default, Some(id.uuid().to_string())),
        _ => (default, None),
    }
}

fn clean_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn image_mime(data: &[u8]) -> &'static str {
    if data.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if data.starts_with(b"GIF8") {
        "image/gif"
    } else if data.starts_with(&[0, 0, 1, 0]) {
        "image/x-icon"
    } else if data.starts_with(b"BM") {
        "image/bmp"
    } else if data.len() > 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        // PNG, which is what KeePass and KeePassXC write
        "image/png"
    }
}

/// The file name without its extension, used when the database has no name.
pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// one match per format reads better than a flattened one here
#[allow(clippy::collapsible_match)]
fn classify(e: &DatabaseOpenError, kdbx3: bool) -> OpenError {
    match e {
        DatabaseOpenError::Key(_) => OpenError::WrongKey,
        DatabaseOpenError::VersionParse(V::InvalidKDBXIdentifier)
        | DatabaseOpenError::VersionParse(V::UnexpectedEof) => OpenError::NotKeePass,
        DatabaseOpenError::VersionParse(_) | DatabaseOpenError::UnsupportedVersion => {
            OpenError::Unsupported
        }
        DatabaseOpenError::UnexpectedEof => OpenError::Corrupt,
        // KDBX 3.1 has no header HMAC: with a wrong key, decryption usually
        // fails on the padding first, the same error a damaged file gives.
        // KDBX 4 checks the key before decrypting, so there it means damage.
        DatabaseOpenError::Cryptography(_) if kdbx3 => OpenError::WrongKeyOrCorrupt,
        DatabaseOpenError::Format(DatabaseFormatError::Kdbx4(k)) => match k {
            Kdbx4OpenError::OuterHeader(H4::OuterCipherConfig(_))
            | Kdbx4OpenError::OuterHeader(H4::KdfConfig(_))
            | Kdbx4OpenError::OuterHeader(H4::CompressionConfig(_)) => OpenError::Unsupported,
            _ => OpenError::Corrupt,
        },
        DatabaseOpenError::Format(DatabaseFormatError::Kdbx3(k)) => match k {
            Kdbx3OpenError::OuterHeader(H3::OuterCipher(_))
            | Kdbx3OpenError::OuterHeader(H3::InnerCipher(_)) => OpenError::Unsupported,
            _ => OpenError::Corrupt,
        },
        DatabaseOpenError::Io(_) => OpenError::FileUnreadable,
        _ => OpenError::Corrupt,
    }
}

struct GroupNode {
    id: GroupId,
    key: String,
    parent: Option<usize>,
    depth: u32,
    kind: GroupKind,
    /// the recycle bin or anything inside it
    in_bin: bool,
    /// `EnableSearching`, inherited from the parent when the file leaves it unset
    searchable: bool,
    /// `EnableAutoType`, inherited the same way
    autotype: bool,
    /// `DefaultAutoTypeSequence`, inherited from the nearest group that has one
    sequence: Option<String>,
}

/// Why an entry cannot be auto-typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutotypeOff {
    /// turned off for the entry or one of its groups
    Disabled,
    /// in the recycle bin
    InRecycleBin,
}

/// An unlocked database. Never written back to disk (that starts in M6).
pub struct Vault {
    db: Database,
    path: PathBuf,
    /// tree order: depth first, the recycle bin last among its siblings
    groups: Vec<GroupNode>,
    group_index: HashMap<String, usize>,
    /// entry id → (keepass id, index of its group in `groups`)
    entries: HashMap<String, (EntryId, usize)>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // never print the database itself
        f.debug_struct("Vault").field("path", &self.path).finish()
    }
}

impl Vault {
    /// Reads and decrypts `path`. Nothing is written anywhere.
    pub fn open(path: &Path, creds: Credentials<'_>) -> Result<Vault, OpenError> {
        let data = fs::read(path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => OpenError::FileNotFound,
            _ => OpenError::FileUnreadable,
        })?;
        let version = match Database::get_version(&mut data.as_slice()) {
            Ok(v) => v,
            Err(DatabaseOpenError::Io(_)) => return Err(OpenError::NotKeePass),
            Err(e) => return Err(classify(&e, false)),
        };
        let kdbx3 = match version {
            DatabaseVersion::KDB(_) => return Err(OpenError::KeePass1),
            DatabaseVersion::KDB2(_) => return Err(OpenError::Unsupported),
            DatabaseVersion::KDB3(_) => true,
            DatabaseVersion::KDB4(_) => false,
        };

        let mut key = DatabaseKey::new();
        if let Some(p) = creds.password.filter(|p| !p.is_empty()) {
            key = key.with_password(p.expose());
        }
        if let Some(path) = creds.key_file {
            let mut file = fs::File::open(path).map_err(|_| OpenError::KeyFileUnreadable)?;
            key = key
                .with_keyfile(&mut file)
                .map_err(|_| OpenError::KeyFileUnreadable)?;
        }
        if key.is_empty() {
            return Err(OpenError::NoKey);
        }

        // keepass-rs panics on some malformed files (M0 issue K9). A damaged
        // file must not take the whole program down.
        let db = match catch_unwind(AssertUnwindSafe(|| Database::parse(&data, key))) {
            Ok(Ok(db)) => db,
            Ok(Err(e)) => return Err(classify(&e, kdbx3)),
            Err(_) => return Err(OpenError::Corrupt),
        };
        Ok(Vault::index(db, path.to_path_buf()))
    }

    fn index(db: Database, path: PathBuf) -> Vault {
        struct Walk {
            bin: Option<GroupId>,
            groups: Vec<GroupNode>,
            entries: HashMap<String, (EntryId, usize)>,
        }
        /// What a group passes on to its children.
        #[derive(Clone, Copy)]
        struct Inherited<'a> {
            in_bin: bool,
            searchable: bool,
            autotype: bool,
            sequence: Option<&'a str>,
        }
        fn walk<'a>(
            w: &mut Walk,
            g: &'a GroupRef<'a>,
            parent: Option<usize>,
            depth: u32,
            from_parent: Inherited<'a>,
        ) {
            let is_bin = w.bin == Some(g.id());
            let here = Inherited {
                in_bin: from_parent.in_bin || is_bin,
                searchable: g.enable_searching.unwrap_or(from_parent.searchable),
                autotype: g.enable_autotype.unwrap_or(from_parent.autotype),
                sequence: g
                    .default_autotype_sequence
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .or(from_parent.sequence),
            };
            let Inherited {
                in_bin,
                searchable,
                autotype,
                sequence,
            } = here;
            let index = w.groups.len();
            w.groups.push(GroupNode {
                id: g.id(),
                key: g.id().uuid().to_string(),
                parent,
                depth,
                kind: if parent.is_none() {
                    GroupKind::Root
                } else if is_bin {
                    GroupKind::RecycleBin
                } else {
                    GroupKind::Normal
                },
                in_bin,
                searchable,
                autotype,
                sequence: sequence.map(str::to_string),
            });
            for e in g.entries() {
                w.entries.insert(e.id().uuid().to_string(), (e.id(), index));
            }
            let mut children: Vec<GroupRef<'_>> = g.groups().collect();
            // stable: keeps the file's order, only moves the bin to the end
            children.sort_by_key(|c| w.bin == Some(c.id()));
            for c in &children {
                walk(w, c, Some(index), depth + 1, here);
            }
        }

        let mut w = Walk {
            bin: db.recycle_bin().map(|g| g.id()),
            groups: Vec::new(),
            entries: HashMap::new(),
        };
        let root = db.root();
        let top = Inherited {
            in_bin: false,
            searchable: true,
            autotype: true,
            sequence: None,
        };
        walk(&mut w, &root, None, 0, top);
        let group_index = w
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| (g.key.clone(), i))
            .collect();
        Vault {
            db,
            path,
            groups: w.groups,
            group_index,
            entries: w.entries,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The database's own name, or the file name when it has none.
    pub fn name(&self) -> String {
        self.db
            .meta
            .database_name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| file_stem(&self.path))
    }

    fn group_ref(&self, node: &GroupNode) -> Option<GroupRef<'_>> {
        self.db.group(node.id)
    }

    /// The root shows every entry outside the recycle bin; any other group
    /// shows its own entries.
    fn entries_in(&self, index: usize) -> Vec<(EntryRef<'_>, usize)> {
        let node = &self.groups[index];
        let wanted = |i: usize| {
            if node.kind == GroupKind::Root {
                !self.groups[i].in_bin
            } else {
                i == index
            }
        };
        let mut out = Vec::new();
        for (i, n) in self.groups.iter().enumerate() {
            if !wanted(i) {
                continue;
            }
            if let Some(g) = self.group_ref(n) {
                let ids: Vec<EntryId> = g.entry_ids().collect();
                out.extend(
                    ids.into_iter()
                        .filter_map(|id| Some((self.db.entry(id)?, i))),
                );
            }
        }
        out
    }

    pub fn groups(&self) -> Vec<GroupView> {
        self.groups
            .iter()
            .enumerate()
            .filter_map(|(i, node)| {
                let g = self.group_ref(node)?;
                let (icon, custom_icon) = icon_of(g.icon(), ICON_FOLDER);
                Some(GroupView {
                    id: node.key.clone(),
                    name: g.name.clone(),
                    parent_id: node.parent.map(|p| self.groups[p].key.clone()),
                    depth: node.depth,
                    kind: node.kind,
                    entry_count: self.entries_in(i).len() as u32,
                    icon,
                    custom_icon,
                    expanded: g.is_expanded,
                    has_children: g.groups().next().is_some(),
                })
            })
            .collect()
    }

    fn summary(&self, e: &EntryRef<'_>, group: usize) -> EntrySummary {
        let text = |k: &str| e.get(k).unwrap_or_default().to_string();
        let (icon, custom_icon) = icon_of(e.icon(), ICON_KEY);
        EntrySummary {
            id: e.id().uuid().to_string(),
            group_id: self.groups[group].key.clone(),
            title: text(fields::TITLE),
            username: text(fields::USERNAME),
            url: text(fields::URL),
            modified: e.times.last_modification.as_ref().map(iso),
            expired: expired(&e.times),
            has_totp: has_totp(e),
            tags: clean_tags(&e.tags),
            icon,
            custom_icon,
        }
    }

    /// `None` when there is no such group.
    pub fn entries(&self, group_id: &str) -> Option<Vec<EntrySummary>> {
        let index = *self.group_index.get(group_id)?;
        Some(
            self.entries_in(index)
                .iter()
                .map(|(e, g)| self.summary(e, *g))
                .collect(),
        )
    }

    /// Searches everything outside the recycle bin and the groups that turn
    /// searching off (features.md §3.4). Best match first, then newest. The
    /// query syntax (field prefixes, fuzzy matching) is in [`crate::search`].
    pub fn search(&self, query: &str) -> Vec<EntrySummary> {
        let query = Query::parse(query);
        let mut hits: Vec<(u32, EntrySummary)> = Vec::new();
        for (i, node) in self.groups.iter().enumerate() {
            if node.in_bin || !node.searchable {
                continue;
            }
            let Some(g) = self.group_ref(node) else {
                continue;
            };
            for e in g.entries() {
                let h = Haystack {
                    title: e.get(fields::TITLE).unwrap_or_default(),
                    username: e.get(fields::USERNAME).unwrap_or_default(),
                    url: e.get(fields::URL).unwrap_or_default(),
                    notes: e.get(fields::NOTES).unwrap_or_default(),
                    tags: &e.tags,
                };
                if let Some(r) = query.rank(&h) {
                    hits.push((r, self.summary(&e, i)));
                }
            }
        }
        hits.sort_by(|(ra, a), (rb, b)| ra.cmp(rb).then_with(|| b.modified.cmp(&a.modified)));
        hits.into_iter().map(|(_, s)| s).collect()
    }

    fn entry_ref(&self, id: &str) -> Option<(EntryRef<'_>, usize)> {
        let (eid, group) = *self.entries.get(id)?;
        Some((self.db.entry(eid)?, group))
    }

    pub fn entry(&self, id: &str) -> Option<EntryDetail> {
        let (e, group) = self.entry_ref(id)?;
        let text = |k: &str| e.get(k).unwrap_or_default().to_string();

        let mut group_path = Vec::new();
        let mut cur = Some(group);
        while let Some(i) = cur {
            if let Some(g) = self.group_ref(&self.groups[i]) {
                group_path.push(g.name.clone());
            }
            cur = self.groups[i].parent;
        }
        group_path.reverse();

        let mut custom: Vec<FieldView> = e
            .fields
            .iter()
            .filter(|(k, _)| !is_standard_field(k) && !is_otp_field(k))
            .map(|(k, v)| FieldView {
                key: k.clone(),
                protected: v.is_protected(),
                value: (!v.is_protected()).then(|| v.get().clone()),
            })
            .collect();
        custom.sort_by(|a, b| a.key.cmp(&b.key));

        let mut attachments: Vec<AttachmentView> = e
            .attachments_named()
            .map(|(name, a)| AttachmentView {
                name: name.to_string(),
                size: a.data.get().len() as u64,
            })
            .collect();
        attachments.sort_by(|a, b| a.name.cmp(&b.name));

        let (icon, custom_icon) = icon_of(e.icon(), ICON_KEY);
        Some(EntryDetail {
            id: id.to_string(),
            group_id: self.groups[group].key.clone(),
            group_path,
            title: text(fields::TITLE),
            username: text(fields::USERNAME),
            url: text(fields::URL),
            notes: text(fields::NOTES),
            tags: clean_tags(&e.tags),
            created: e.times.creation.as_ref().map(iso),
            modified: e.times.last_modification.as_ref().map(iso),
            expires: expires(&e.times).map(iso),
            expired: expired(&e.times),
            has_password: e.get(fields::PASSWORD).is_some_and(|p| !p.is_empty()),
            has_totp: has_totp(&e),
            fields: custom,
            attachments,
            icon,
            custom_icon,
        })
    }

    /// One field's value, for `reveal_field` (and, from M3, copying). The
    /// standard keys follow KeePass; anything else is a custom field name.
    /// TOTP secrets are never handed out.
    pub fn field(&self, id: &str, key: &str) -> Option<Secret> {
        if is_otp_field(key) {
            return None;
        }
        let (e, _) = self.entry_ref(id)?;
        match e.fields.get(key) {
            Some(v) => Some(Secret::new(v.get().clone())),
            None if is_standard_field(key) => Some(Secret::default()),
            None => None,
        }
    }

    /// Whether a field holds a secret: the password, or a protected custom
    /// field. `None` when the entry or field does not exist.
    pub fn is_secret_field(&self, id: &str, key: &str) -> Option<bool> {
        let (e, _) = self.entry_ref(id)?;
        if key == fields::PASSWORD {
            return Some(true);
        }
        if is_standard_field(key) {
            return Some(false);
        }
        e.fields.get(key).map(|v| v.is_protected())
    }

    pub fn has_totp(&self, id: &str) -> Option<bool> {
        self.entry_ref(id).map(|(e, _)| has_totp(&e))
    }

    /// The current TOTP code. `None` when there is no such entry.
    pub fn totp(&self, id: &str) -> Option<Result<TotpView, TotpError>> {
        let (e, _) = self.entry_ref(id)?;
        Some(Totp::from_fields(|k| e.get(k)).map(|t| {
            let c = t.now();
            TotpView {
                code: c.code,
                period: c.period,
                remaining: c.remaining,
            }
        }))
    }

    /// What copying a field puts on the clipboard, and whether it is a
    /// secret (cleared after a while, kept out of clipboard history). `@totp`
    /// is the current code. `None` when the entry or field does not exist.
    pub fn copy_value(&self, id: &str, key: &str) -> Option<Result<(Secret, bool), TotpError>> {
        if key == fields::TOTP {
            return self
                .totp(id)
                .map(|r| r.map(|t| (Secret::new(t.code), true)));
        }
        let secret = self.is_secret_field(id, key)?;
        Some(Ok((self.field(id, key)?, secret)))
    }

    /// Entries that fit a window, with the sequence to type for each, in tree
    /// order (features.md §3.10). An entry fits through its first matching
    /// window association; else, in a browser, when the page is on the
    /// entry URL's site (`page_url`); else, for an entry without
    /// associations, when the window title contains its title. Skips the
    /// recycle bin and entries with auto-type turned off.
    pub fn autotype_matches(
        &self,
        window_title: &str,
        page_url: Option<&str>,
    ) -> Vec<(EntrySummary, String)> {
        let title = window_title.to_lowercase();
        let mut found = Vec::new();
        for (i, node) in self.groups.iter().enumerate() {
            if node.in_bin || !node.autotype {
                continue;
            }
            let Some(g) = self.group_ref(node) else {
                continue;
            };
            for e in g.entries() {
                let Some(at) = autotype_of(&e) else { continue };
                let by_window = at
                    .associations
                    .iter()
                    .find(|a| autotype::window_matches(&a.window, window_title))
                    .map(|a| self.sequence_for(&e, i, Some(&a.sequence)));
                let by_url = || {
                    let url = e.get(fields::URL).unwrap_or_default();
                    page_url
                        .is_some_and(|p| autotype::url_matches(url, p))
                        .then(|| self.sequence_for(&e, i, None))
                };
                let by_title = || {
                    let own = e
                        .get(fields::TITLE)
                        .unwrap_or_default()
                        .trim()
                        .to_lowercase();
                    (at.associations.is_empty() && !own.is_empty() && title.contains(&own))
                        .then(|| self.sequence_for(&e, i, None))
                };
                let sequence = by_window.or_else(by_url).or_else(by_title);
                if let Some(s) = sequence {
                    found.push((self.summary(&e, i), s));
                }
            }
        }
        found
    }

    /// The sequence to type for one entry chosen by the user (quick search,
    /// main window): the first window association that fits `window_title`,
    /// otherwise the entry's or its groups' default. `None` when there is no
    /// such entry.
    pub fn autotype_sequence(
        &self,
        id: &str,
        window_title: &str,
    ) -> Option<Result<String, AutotypeOff>> {
        let (e, group) = self.entry_ref(id)?;
        let node = &self.groups[group];
        if node.in_bin {
            return Some(Err(AutotypeOff::InRecycleBin));
        }
        let Some(at) = autotype_of(&e).filter(|_| node.autotype) else {
            return Some(Err(AutotypeOff::Disabled));
        };
        let assoc = at
            .associations
            .iter()
            .find(|a| autotype::window_matches(&a.window, window_title));
        Some(Ok(self.sequence_for(
            &e,
            group,
            assoc.map(|a| a.sequence.as_str()),
        )))
    }

    /// Association sequence, else the entry's default, else the groups',
    /// else KeePass's default. Empty ones do not count.
    fn sequence_for(&self, e: &EntryRef<'_>, group: usize, assoc: Option<&str>) -> String {
        let own = |s: Option<&str>| {
            s.map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        own(assoc)
            .or_else(|| {
                own(e
                    .autotype
                    .as_ref()
                    .and_then(|a| a.default_sequence.as_deref()))
            })
            .or_else(|| self.groups[group].sequence.clone())
            .unwrap_or_else(|| DEFAULT_SEQUENCE.to_string())
    }

    /// The entry's URL, for opening in a browser.
    pub fn url(&self, id: &str) -> Option<String> {
        let (e, _) = self.entry_ref(id)?;
        Some(e.get(fields::URL).unwrap_or_default().trim().to_string())
    }

    pub fn custom_icons(&self) -> Vec<CustomIconView> {
        self.db
            .iter_all_custom_icons()
            .map(|i| CustomIconView {
                id: i.id().uuid().to_string(),
                mime: image_mime(&i.data),
                data: B64.encode(&i.data),
            })
            .collect()
    }
}

/// The entry's auto-type settings when it may be auto-typed. An entry
/// without an `AutoType` element gets the defaults (enabled, no associations).
fn autotype_of(e: &EntryRef<'_>) -> Option<keepass::db::AutoType> {
    match &e.autotype {
        Some(a) if !a.enabled => None,
        Some(a) => Some(a.clone()),
        None => Some(keepass::db::AutoType::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
    }

    fn password() -> Secret {
        Secret::new("mikoshi-test")
    }

    fn open(file: &str, pw: bool, key: Option<&str>) -> Result<Vault, OpenError> {
        let p = password();
        let key = key.map(fixture);
        Vault::open(
            &fixture(file),
            Credentials {
                password: pw.then_some(&p),
                key_file: key.as_deref(),
            },
        )
    }

    fn open_ok() -> Vault {
        open("kxc/kdbx41-pw.kdbx", true, None).expect("fixture opens")
    }

    #[test]
    fn opens_every_keepassxc_fixture() {
        let cases: &[(&str, bool, Option<&str>)] = &[
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
        for (file, pw, key) in cases {
            let v = open(file, *pw, *key).unwrap_or_else(|e| panic!("{file}: {e:?}"));
            assert!(!v.groups().is_empty(), "{file}");
            assert!(!v.search("").is_empty(), "{file}");
        }
    }

    #[test]
    fn tells_failures_apart() {
        let wrong = Secret::new("not-the-password");
        let creds = |p| Credentials {
            password: Some(p),
            key_file: None,
        };
        assert_eq!(
            Vault::open(&fixture("kxc/kdbx41-pw.kdbx"), creds(&wrong)).unwrap_err(),
            OpenError::WrongKey
        );
        let e31 = Vault::open(&fixture("kxc/kdbx31-pw.kdbx"), creds(&wrong)).unwrap_err();
        assert!(matches!(
            e31,
            OpenError::WrongKey | OpenError::WrongKeyOrCorrupt
        ));
        // the password is right but the key file is missing
        assert_eq!(
            open("kxc/kdbx41-pw-keyxml2.kdbx", true, None).unwrap_err(),
            OpenError::WrongKey
        );
        assert_eq!(
            open("kxc/kdbx41-pw.kdbx", false, None).unwrap_err(),
            OpenError::NoKey
        );
        assert_eq!(
            open("does-not-exist.kdbx", true, None).unwrap_err(),
            OpenError::FileNotFound
        );
        assert_eq!(
            open("kxc/kdbx41-pw.kdbx", true, Some("keys/missing.key")).unwrap_err(),
            OpenError::KeyFileUnreadable
        );

        let dir = std::env::temp_dir().join(format!("mikoshi-vault-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, data: &[u8]| {
            let p = dir.join(name);
            fs::write(&p, data).unwrap();
            p
        };
        let pw = password();
        let try_open = |p: &Path| Vault::open(p, creds(&pw)).unwrap_err();

        assert_eq!(try_open(&write("empty.kdbx", b"")), OpenError::NotKeePass);
        assert_eq!(
            try_open(&write("text.kdbx", b"this is not a database at all")),
            OpenError::NotKeePass
        );
        // KeePass 1 signature: 0x9AA2D903 0xB54BFB65
        let mut kdb = vec![0x03, 0xD9, 0xA2, 0x9A, 0x65, 0xFB, 0x4B, 0xB5];
        kdb.extend_from_slice(&[0u8; 120]);
        assert_eq!(try_open(&write("old.kdb", &kdb)), OpenError::KeePass1);

        let full = fs::read(fixture("kxc/kdbx41-pw.kdbx")).unwrap();
        let cut = write("cut.kdbx", &full[..full.len() / 2]);
        assert!(matches!(
            try_open(&cut),
            OpenError::Corrupt | OpenError::WrongKey
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn views_never_contain_secrets() {
        let v = open_ok();
        let mut json = serde_json::to_string(&v.groups()).unwrap();
        for g in v.groups() {
            for s in v.entries(&g.id).unwrap() {
                json += &serde_json::to_string(&s).unwrap();
                json += &serde_json::to_string(&v.entry(&s.id).unwrap()).unwrap();
            }
        }
        json += &serde_json::to_string(&v.search("")).unwrap();

        let mut checked = 0;
        for e in v.db.iter_all_entries() {
            for (k, value) in &e.fields {
                let secret = value.is_protected() || is_otp_field(k);
                let text = value.get();
                // very short values could match anything by chance
                if secret && text.chars().count() >= 6 {
                    assert!(!json.contains(text.as_str()), "{k} leaked");
                    checked += 1;
                }
            }
        }
        assert!(checked > 10, "the fixture should have secrets to check");
    }

    #[test]
    fn recycle_bin_comes_last_and_is_not_searched() {
        let v = open_ok();
        let groups = v.groups();
        let root = groups.iter().find(|g| g.kind == GroupKind::Root).unwrap();
        let bin = groups
            .iter()
            .position(|g| g.kind == GroupKind::RecycleBin)
            .expect("the fixture has a recycle bin");
        assert!(
            groups[bin + 1..]
                .iter()
                .all(|g| g.depth > groups[bin].depth),
            "only the bin's own subgroups may follow it"
        );

        let bin_ids: Vec<&str> = groups[bin..]
            .iter()
            .take_while(|g| g.depth >= groups[bin].depth)
            .map(|g| g.id.as_str())
            .collect();
        for hit in v.search("") {
            assert!(!bin_ids.contains(&hit.group_id.as_str()));
        }
        for e in v.entries(&root.id).unwrap() {
            assert!(!bin_ids.contains(&e.group_id.as_str()));
        }
    }

    #[test]
    fn otp_secrets_are_hidden_and_detected() {
        let v = open_ok();
        let totp: Vec<EntrySummary> = v.search("").into_iter().filter(|e| e.has_totp).collect();
        assert!(!totp.is_empty(), "the fixture has TOTP entries");
        for s in &totp {
            let d = v.entry(&s.id).unwrap();
            assert!(d.fields.iter().all(|f| !is_otp_field(&f.key)));
            assert!(v.field(&s.id, "otp").is_none());
        }
    }

    #[test]
    fn computes_totp_for_every_fixture_format() {
        // otpauth with defaults, SHA-256 + 8 digits + 60 s, lower-case and
        // unpadded secrets, KeePass 2 native fields: all readable since M3
        let v = open_ok();
        let totp: Vec<EntrySummary> = v.search("").into_iter().filter(|e| e.has_totp).collect();
        assert!(totp.len() >= 6, "the fixture has six TOTP formats");
        for s in &totp {
            let code = v
                .totp(&s.id)
                .unwrap()
                .unwrap_or_else(|e| panic!("{}: {e:?}", s.title));
            assert!(code.code.len() == 6 || code.code.len() == 8, "{}", s.title);
            assert!(code.code.bytes().all(|b| b.is_ascii_digit()));
            assert!(code.remaining >= 1 && code.remaining <= code.period);
            let (copied, secret) = v.copy_value(&s.id, fields::TOTP).unwrap().unwrap();
            assert!(secret);
            assert_eq!(copied.expose().len(), code.code.len());
        }
        let plain = v.search("").into_iter().find(|e| !e.has_totp).unwrap();
        assert_eq!(v.totp(&plain.id).unwrap().unwrap_err(), TotpError::Missing);
    }

    fn id_of(v: &Vault, title: &str) -> String {
        v.search(&format!("title:\"{title}\""))
            .into_iter()
            .find(|e| e.title == title)
            .map(|e| e.id)
            .unwrap_or_else(|| {
                // not searchable (recycle bin, EnableSearching off): walk the index
                v.entries
                    .keys()
                    .find(|id| v.entry(id).is_some_and(|d| d.title == title))
                    .cloned()
                    .expect(title)
            })
    }

    #[test]
    fn autotype_matches_window_associations() {
        let v = open_ok();
        let titles = |w: &str| -> Vec<(String, String)> {
            v.autotype_matches(w, None)
                .into_iter()
                .map(|(e, s)| (e.title, s))
                .collect()
        };
        // wildcard, with the association's own sequence
        assert_eq!(
            titles("notes.txt - Notepad"),
            [("AutoType Custom".into(), "{PASSWORD}{ENTER}".into())]
        );
        // wildcard with an empty sequence: the entry's default
        assert_eq!(
            titles("Login - Example - Microsoft Edge"),
            [(
                "AutoType Custom".into(),
                "{USERNAME}{TAB}{TAB}{PASSWORD}{ENTER}".into()
            )]
        );
        // regular expression
        assert_eq!(
            titles("Bank of Example - Google Chrome"),
            [(
                "AutoType Custom".into(),
                "{S:Secret Field}{DELAY 100}{ENTER}".into()
            )]
        );
        // no associations: the window title contains the entry title
        assert_eq!(
            titles("Sign in - full fields - Edge"),
            [("Full Fields".into(), DEFAULT_SEQUENCE.into())]
        );
        assert!(titles("Something else entirely").is_empty());
    }

    #[test]
    fn autotype_matches_browser_pages_by_url() {
        let v = open_ok();
        let titles = |url: &str| -> Vec<String> {
            v.autotype_matches("Example Domain - Google Chrome", Some(url))
                .into_iter()
                .map(|(e, s)| {
                    assert!(autotype::parse(&s).is_ok());
                    e.title
                })
                .collect()
        };
        // Full Fields has https://example.com/login?a=1&b=2
        assert!(titles("accounts.example.com/signin").contains(&"Full Fields".to_string()));
        assert!(titles("https://example.com").contains(&"Full Fields".to_string()));
        assert!(!titles("example.org").contains(&"Full Fields".to_string()));
        // no page address, no URL matching
        assert!(
            !v.autotype_matches("Example Domain - Google Chrome", None)
                .iter()
                .any(|(e, _)| e.title == "Full Fields")
        );
    }

    #[test]
    fn autotype_skips_disabled_entries_groups_and_the_bin() {
        let v = open_ok();
        for title in ["AutoType Disabled", "Deep Entry", "Deleted Entry"] {
            assert!(
                v.autotype_matches(&format!("{title} - Login"), None)
                    .is_empty(),
                "{title}"
            );
        }
        let seq = |t: &str, w: &str| v.autotype_sequence(&id_of(&v, t), w).unwrap();
        assert_eq!(seq("AutoType Disabled", "x"), Err(AutotypeOff::Disabled));
        // EnableAutoType=False on Level 3
        assert_eq!(seq("Deep Entry", "x"), Err(AutotypeOff::Disabled));
        assert_eq!(seq("Deleted Entry", "x"), Err(AutotypeOff::InRecycleBin));
        assert!(v.autotype_sequence("no such entry", "x").is_none());
    }

    #[test]
    fn autotype_sequence_for_a_chosen_entry() {
        let v = open_ok();
        let seq = |t: &str, w: &str| v.autotype_sequence(&id_of(&v, t), w).unwrap().unwrap();
        // a fitting association still wins
        assert_eq!(seq("AutoType Custom", "a - Notepad"), "{PASSWORD}{ENTER}");
        // otherwise the entry's default
        assert_eq!(
            seq("AutoType Custom", "Some Window"),
            "{USERNAME}{TAB}{TAB}{PASSWORD}{ENTER}"
        );
        // no AutoType settings: the group's default, then KeePass's
        assert_eq!(seq("Full Fields", "Some Window"), DEFAULT_SEQUENCE);
        // every fixture sequence parses
        for (t, w) in [
            ("AutoType Custom", "a - Notepad"),
            ("AutoType Custom", "Bank x"),
            ("AutoType Custom", "y"),
        ] {
            assert!(autotype::parse(&seq(t, w)).is_ok(), "{t} / {w}");
        }
    }

    #[test]
    fn copy_values_know_what_is_secret() {
        let v = open_ok();
        let s = v.search("Full Fields").into_iter().next().unwrap();
        let (pw, secret) = v.copy_value(&s.id, fields::PASSWORD).unwrap().unwrap();
        assert!(secret && !pw.is_empty());
        let (_, secret) = v.copy_value(&s.id, fields::USERNAME).unwrap().unwrap();
        assert!(!secret);
        assert!(v.copy_value(&s.id, "no such field").is_none());
        assert!(v.url(&s.id).is_some_and(|u| u.contains("example.com")));
    }

    #[test]
    fn protected_fields_are_revealed_one_at_a_time() {
        let v = open_ok();
        let s = v
            .search("")
            .into_iter()
            .find(|s| v.entry(&s.id).is_some_and(|d| d.has_password))
            .unwrap();
        let d = v.entry(&s.id).unwrap();
        assert!(v.field(&s.id, fields::PASSWORD).is_some());
        assert_eq!(v.is_secret_field(&s.id, fields::PASSWORD), Some(true));
        assert_eq!(v.is_secret_field(&s.id, fields::USERNAME), Some(false));
        for f in d.fields {
            assert_eq!(f.value.is_none(), f.protected);
        }
        assert!(v.field("no-such-entry", fields::PASSWORD).is_none());
    }
}
