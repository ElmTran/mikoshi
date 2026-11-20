//! Creating and editing entries, and their history (features.md §3.8).
//!
//! Edits change the tree in memory and mark the vault as unsaved; nothing is
//! written until [`Vault::save`]. Each applied edit puts the old version into
//! the entry's history once, trimmed to the database's own limits.
//!
//! [`EditableEntry`] is the one view that carries secrets: the editor needs
//! the values to show and change them (architecture.md, rule 3). Everything
//! the editor sends back comes as [`EntryEdit`], secrets as [`Secret`].

use std::collections::HashSet;

use kdbx::xml::Element;
use kdbx::{EntryMut, Timestamp};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{Fields, Vault, is_otp_field, is_standard_field, iso};
use crate::Secret;
use crate::model::{EntryDetail, fields};
use crate::totp::{self, Algorithm, Encoder, Totp};

/// Why an edit was refused. Carries no user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    /// no such entry, group or history version
    NotFound,
    /// a custom field name that is empty, used twice, or taken by a standard
    /// or TOTP field
    FieldName,
    /// the TOTP settings cannot be used
    TotpInvalid,
    /// the expiry time cannot be read
    Expiry,
    /// new entries and groups do not go into the recycle bin
    RecycleBin,
    /// not possible for this item (the root group, a group into itself, ...)
    NotAllowed,
    /// a group or attachment name that is empty
    EmptyName,
}

// ---------------------------------------------------------------- to the editor

/// An entry with every value, for the editor. **Holds secrets.**
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableEntry {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub username: String,
    pub password: String,
    pub url: String,
    pub notes: String,
    pub tags: Vec<String>,
    /// ISO 8601 in UTC; `None` means never
    pub expires: Option<String>,
    /// custom fields in the file's order, TOTP fields left out
    pub fields: Vec<EditableField>,
    /// the TOTP settings, without the secret
    pub totp: Option<TotpSummary>,
    /// KeePass standard icon, 0–68
    pub icon: u32,
    /// custom icon id, shown instead of `icon`
    pub custom_icon: Option<String>,
}

impl std::fmt::Debug for EditableEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EditableEntry([redacted])")
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableField {
    pub key: String,
    pub value: String,
    pub protected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpSummary {
    /// `false` when settings are there but cannot be used
    pub valid: bool,
    /// `SHA1`, `SHA256` or `SHA512`
    pub algorithm: Option<&'static str>,
    pub digits: Option<u32>,
    pub period: Option<u64>,
    pub steam: bool,
}

/// One older version of an entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    /// position in the file's history, oldest first; what the history
    /// commands take
    pub index: usize,
    pub modified: Option<String>,
    pub title: String,
    pub username: String,
}

// ---------------------------------------------------------------- from the editor

/// The whole entry as the editor wants it to be.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryEdit {
    pub title: String,
    pub username: String,
    pub password: Secret,
    pub url: String,
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// ISO 8601; `None` means never
    #[serde(default)]
    pub expires: Option<String>,
    /// the custom fields wanted, in order. Fields the entry has that are not
    /// named here (as `original`) are removed.
    #[serde(default)]
    pub fields: Vec<FieldEdit>,
    #[serde(default)]
    pub totp: TotpEdit,
    /// a KeePass standard icon, 0–68; `None` leaves the icon as it is
    #[serde(default)]
    pub icon: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldEdit {
    /// the field's name before the edit; `None` for a new field
    #[serde(default)]
    pub original: Option<String>,
    pub key: String,
    pub value: Secret,
    pub protected: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TotpEdit {
    #[default]
    Unchanged,
    Remove,
    /// a pasted `otpauth://` URI
    Uri {
        uri: Secret,
    },
    /// a Base32 secret typed in by hand
    #[serde(rename_all = "camelCase")]
    Manual {
        secret: Secret,
        #[serde(default = "default_algorithm")]
        algorithm: String,
        #[serde(default = "default_digits")]
        digits: u32,
        #[serde(default = "default_period")]
        period: u64,
    },
}

fn default_algorithm() -> String {
    "SHA1".into()
}
fn default_digits() -> u32 {
    6
}
fn default_period() -> u64 {
    30
}

/// An edit that passed every check, ready to apply.
struct Checked<'a> {
    edit: &'a EntryEdit,
    expires: Option<Timestamp>,
    /// `Some(None)`: remove TOTP; `Some(Some(v))`: set `otp` to `v`
    otp: Option<Option<Zeroizing<String>>>,
}

fn check(edit: &EntryEdit) -> Result<Checked<'_>, EditError> {
    let mut seen = HashSet::new();
    for f in &edit.fields {
        let k = f.key.as_str();
        if k.trim().is_empty() || k != k.trim() || is_standard_field(k) || is_otp_field(k) {
            return Err(EditError::FieldName);
        }
        if !seen.insert(k) {
            return Err(EditError::FieldName);
        }
    }
    let mut originals = HashSet::new();
    for o in edit.fields.iter().filter_map(|f| f.original.as_deref()) {
        if !originals.insert(o) {
            return Err(EditError::FieldName);
        }
    }
    let expires = match edit.expires.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(t) => Some(Timestamp::parse_iso(t).ok_or(EditError::Expiry)?),
    };
    let otp = match &edit.totp {
        TotpEdit::Unchanged => None,
        TotpEdit::Remove => Some(None),
        TotpEdit::Uri { uri } => {
            let uri = uri.expose().trim();
            Totp::parse_otp(uri).map_err(|_| EditError::TotpInvalid)?;
            Some(Some(Zeroizing::new(uri.to_string())))
        }
        TotpEdit::Manual {
            secret,
            algorithm,
            digits,
            period,
        } => {
            let alg = match algorithm.to_ascii_uppercase().replace('-', "").as_str() {
                "SHA1" => Algorithm::Sha1,
                "SHA256" => Algorithm::Sha256,
                "SHA512" => Algorithm::Sha512,
                _ => return Err(EditError::TotpInvalid),
            };
            let uri = Totp::manual_uri(
                secret.expose(),
                alg,
                *digits,
                *period,
                &edit.title,
                &edit.username,
            )
            .map_err(|_| EditError::TotpInvalid)?;
            Some(Some(uri))
        }
    };
    Ok(Checked { edit, expires, otp })
}

/// `(key, protected)` of each field, in the file's order.
fn string_fields(el: &Element) -> Vec<(String, bool)> {
    el.children_named("String")
        .map(|s| {
            let key = s.child_text("Key").unwrap_or_default().into_owned();
            let protected = s
                .child("Value")
                .is_some_and(|v| v.secret().is_some() || v.is_protected());
            (key, protected)
        })
        .collect()
}

fn value_of(el: &Element, key: &str) -> Option<(Zeroizing<Vec<u8>>, bool)> {
    let s = el
        .children_named("String")
        .find(|s| s.child_text("Key").as_deref() == Some(key))?;
    let Some(v) = s.child("Value") else {
        return Some((Zeroizing::new(Vec::new()), false));
    };
    Some(match v.secret() {
        Some(b) => (Zeroizing::new(b.to_vec()), true),
        None => (Zeroizing::new(v.text().as_bytes().to_vec()), false),
    })
}

/// Sets a field only when its value or protection differs, so untouched
/// fields stay exactly as they were in the file.
fn set_if_changed(e: &mut EntryMut<'_>, key: &str, value: &str, protected: bool) {
    let same = value_of(e.element(), key)
        .is_some_and(|(v, p)| v.as_slice() == value.as_bytes() && p == protected);
    if !same {
        e.set_field(key, value, protected);
    }
}

/// Makes `e` what the edit asks for. `protect` says whether a standard field
/// the entry does not have yet is protected.
fn apply(e: &mut EntryMut<'_>, c: &Checked<'_>, protect: &dyn Fn(&str) -> bool) {
    let edit = c.edit;
    if let Some(icon) = edit.icon.filter(|i| *i < 69) {
        let el = e.element();
        let same = el
            .child_text("IconID")
            .and_then(|t| t.trim().parse::<u32>().ok())
            == Some(icon)
            && el.child("CustomIconUUID").is_none();
        if !same {
            e.set_icon(icon);
        }
    }
    let existing = string_fields(e.element());
    let protected_now = |k: &str| existing.iter().find(|(x, _)| x == k).map(|(_, p)| *p);

    for (key, value) in [
        (fields::TITLE, edit.title.as_str()),
        (fields::USERNAME, edit.username.as_str()),
        (fields::PASSWORD, edit.password.expose()),
        (fields::URL, edit.url.as_str()),
        (fields::NOTES, edit.notes.as_str()),
    ] {
        match protected_now(key) {
            Some(p) => set_if_changed(e, key, value, p),
            None if value.is_empty() => {}
            None => e.set_field(key, value, protect(key)),
        }
    }

    // custom fields: drop the ones no longer wanted, rename, then set values
    let kept: HashSet<&str> = edit
        .fields
        .iter()
        .filter_map(|f| f.original.as_deref())
        .collect();
    for (k, _) in &existing {
        if !is_standard_field(k) && !is_otp_field(k) && !kept.contains(k.as_str()) {
            e.remove_field(k);
        }
    }
    let renames: Vec<(&str, &str)> = edit
        .fields
        .iter()
        .filter_map(|f| f.original.as_deref().map(|o| (o, f.key.as_str())))
        .filter(|(o, k)| o != k)
        .collect();
    // through temporary names, so swapping two names works
    let temp = |i: usize| format!("\u{1}mikoshi-rename-{i}");
    for (i, (o, _)) in renames.iter().enumerate() {
        e.rename_field(o, &temp(i));
    }
    for (i, (_, k)) in renames.iter().enumerate() {
        e.rename_field(&temp(i), k);
    }
    for f in &edit.fields {
        set_if_changed(e, &f.key, f.value.expose(), f.protected);
    }

    let tags: Vec<String> = edit
        .tags
        .iter()
        .flat_map(|t| t.split([';', ',']))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let tags_now: Vec<String> = e
        .element()
        .child_text("Tags")
        .map(|t| {
            t.split([';', ','])
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if tags != tags_now {
        e.set_tags(&tags);
    }

    let times = e.element().child("Times");
    let expires_now = times
        .filter(|t| {
            t.child_text("Expires")
                .is_some_and(|v| kdbx::xml::is_true(&v))
        })
        .and_then(|t| t.child_text("ExpiryTime"))
        .and_then(|t| Timestamp::parse(&t));
    if c.expires != expires_now {
        e.set_expiry(c.expires);
    }

    if let Some(otp) = &c.otp {
        let new = otp.as_deref().map(String::as_str);
        let old_otp_only = existing
            .iter()
            .filter(|(k, _)| is_otp_field(k))
            .all(|(k, _)| k == "otp");
        let unchanged = old_otp_only
            && match (new, value_of(e.element(), "otp")) {
                (Some(n), Some((v, true))) => v.as_slice() == n.as_bytes(),
                (None, None) => true,
                _ => false,
            };
        if !unchanged {
            for (k, _) in &existing {
                if is_otp_field(k) {
                    e.remove_field(k);
                }
            }
            if let Some(v) = new {
                e.set_field("otp", v, true);
            }
        }
    }
}

impl Vault {
    /// Whether there are edits not saved yet.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether a standard field is protected in new entries (the database's
    /// `MemoryProtection` settings; the password always is).
    fn protect_rule(&self) -> impl Fn(&str) -> bool + use<> {
        let mp = self.db.meta().element().child("MemoryProtection");
        let flag = |name: &str, default: bool| {
            mp.and_then(|m| m.child_text(name))
                .map_or(default, |v| kdbx::xml::is_true(&v))
        };
        let (title, user, url, notes) = (
            flag("ProtectTitle", false),
            flag("ProtectUserName", false),
            flag("ProtectURL", false),
            flag("ProtectNotes", false),
        );
        move |k: &str| match k {
            fields::PASSWORD => true,
            fields::TITLE => title,
            fields::USERNAME => user,
            fields::URL => url,
            fields::NOTES => notes,
            _ => false,
        }
    }

    /// Everything the editor shows, secrets included.
    pub fn editable(&self, id: &str) -> Option<EditableEntry> {
        let (e, group) = self.entry_ref(id)?;
        let f = Fields::of(&e);
        let totp = totp::has_totp(|k| f.get(k)).then(|| match Totp::from_fields(|k| f.get(k)) {
            Ok(t) => TotpSummary {
                valid: true,
                algorithm: Some(match t.algorithm() {
                    Algorithm::Sha1 => "SHA1",
                    Algorithm::Sha256 => "SHA256",
                    Algorithm::Sha512 => "SHA512",
                }),
                digits: match t.encoder() {
                    Encoder::Digits(d) => Some(d),
                    Encoder::Steam => None,
                },
                period: Some(t.period()),
                steam: t.encoder() == Encoder::Steam,
            },
            Err(_) => TotpSummary {
                valid: false,
                algorithm: None,
                digits: None,
                period: None,
                steam: false,
            },
        });
        let (icon, custom_icon) = super::icon_of(e.icon_id(), e.custom_icon(), super::ICON_KEY);
        Some(EditableEntry {
            icon,
            custom_icon,
            id: id.to_string(),
            group_id: self.groups[group].key.clone(),
            title: f.text(fields::TITLE),
            username: f.text(fields::USERNAME),
            password: f.text(fields::PASSWORD),
            url: f.text(fields::URL),
            notes: f.text(fields::NOTES),
            tags: e.tags(),
            expires: e.times().expires_at().map(iso),
            fields: e
                .fields()
                .filter(|x| !is_standard_field(&x.key()) && !is_otp_field(&x.key()))
                .map(|x| EditableField {
                    key: x.key().into_owned(),
                    value: x.value().into_owned(),
                    protected: x.protected(),
                })
                .collect(),
            totp,
        })
    }

    /// Applies an edit. The old version goes into the history first. Returns
    /// `false`, changing nothing, when the edit changes nothing.
    pub fn update_entry(&mut self, id: &str, edit: &EntryEdit) -> Result<bool, EditError> {
        let c = check(edit)?;
        let (uuid, _) = *self.entries.get(id).ok_or(EditError::NotFound)?;
        let protect = self.protect_rule();
        let current = self.db.entry(uuid).ok_or(EditError::NotFound)?.element();

        // try it on a copy first: an edit that changes nothing leaves no
        // history version and no unsaved mark
        let mut before = current.clone();
        before.remove_child("History");
        let mut after = before.clone();
        for f in &edit.fields {
            if let Some(o) = &f.original
                && !string_fields(&before).iter().any(|(k, _)| k == o)
            {
                return Err(EditError::NotFound);
            }
        }
        apply(&mut EntryMut::new(&mut after), &c, &protect);
        if after == before {
            return Ok(false);
        }

        let mut e = self.db.entry_mut(uuid).ok_or(EditError::NotFound)?;
        e.push_history();
        apply(&mut e, &c, &protect);
        e.touch();
        self.db.maintain_history(uuid);
        self.dirty = true;
        Ok(true)
    }

    /// Adds an entry to a group (the root when `group_id` is the root) and
    /// returns its id.
    pub fn create_entry(&mut self, group_id: &str, edit: &EntryEdit) -> Result<String, EditError> {
        let c = check(edit)?;
        if edit.fields.iter().any(|f| f.original.is_some()) {
            return Err(EditError::NotFound);
        }
        let index = *self.group_index.get(group_id).ok_or(EditError::NotFound)?;
        if self.groups[index].in_bin {
            return Err(EditError::RecycleBin);
        }
        let protect = self.protect_rule();
        let uuid = self
            .db
            .add_entry(self.groups[index].id)
            .ok_or(EditError::NotFound)?;
        let mut e = self.db.entry_mut(uuid).ok_or(EditError::NotFound)?;
        apply(&mut e, &c, &protect);
        self.dirty = true;
        self.relayout();
        Ok(uuid.to_string())
    }

    fn history_ref(&self, id: &str, index: usize) -> Option<(kdbx::Entry<'_>, usize)> {
        let (e, group) = self.entry_ref(id)?;
        Some((e.history().nth(index)?, group))
    }

    /// The entry's older versions, oldest first.
    pub fn history(&self, id: &str) -> Option<Vec<HistoryItem>> {
        let (e, _) = self.entry_ref(id)?;
        Some(
            e.history()
                .enumerate()
                .map(|(index, h)| HistoryItem {
                    index,
                    modified: h.times().last_modification.map(iso),
                    title: h.get(fields::TITLE).unwrap_or_default().into_owned(),
                    username: h.get(fields::USERNAME).unwrap_or_default().into_owned(),
                })
                .collect(),
        )
    }

    /// An older version, shown like an entry (no secrets).
    pub fn history_entry(&self, id: &str, index: usize) -> Option<EntryDetail> {
        let (h, group) = self.history_ref(id, index)?;
        Some(self.detail(&h, id, group))
    }

    /// One field of an older version, for showing it. TOTP secrets are never
    /// handed out.
    pub fn history_field(&self, id: &str, index: usize, key: &str) -> Option<Secret> {
        if is_otp_field(key) {
            return None;
        }
        let (h, _) = self.history_ref(id, index)?;
        match h.field(key) {
            Some(v) => Some(Secret::new(v.value().into_owned())),
            None if is_standard_field(key) => Some(Secret::default()),
            None => None,
        }
    }

    /// Makes an older version the current one; the current version goes into
    /// the history.
    pub fn restore_history(&mut self, id: &str, index: usize) -> Result<(), EditError> {
        let (uuid, _) = *self.entries.get(id).ok_or(EditError::NotFound)?;
        if !self.db.restore_history(uuid, index) {
            return Err(EditError::NotFound);
        }
        self.dirty = true;
        Ok(())
    }
}
