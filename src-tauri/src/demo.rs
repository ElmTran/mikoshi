//! Demo database for the M1 UI sample. Replaced by a real KDBX-backed vault
//! in M2; the commands and the view types stay the same.

use chrono::{Local, NaiveDateTime};
use mikoshi_core::Secret;
use mikoshi_core::model::{
    AttachmentView, EntryDetail, EntrySummary, FieldView, GroupKind, GroupView, TotpView, fields,
};
use mikoshi_core::search::{Haystack, rank};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

const DEMO_JSON: &str = include_str!("../demo/vault.json");
const ISO: &str = "%Y-%m-%dT%H:%M:%S";

#[derive(Deserialize)]
struct DemoFile {
    name: String,
    file: String,
    path: String,
    groups: Vec<DemoGroup>,
    entries: Vec<DemoEntry>,
}

#[derive(Deserialize)]
struct DemoGroup {
    id: String,
    name: String,
    parent: Option<String>,
    kind: String,
}

#[derive(Deserialize)]
struct DemoEntry {
    id: String,
    group: String,
    title: String,
    username: String,
    password: Secret,
    url: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    otp: bool,
    created: String,
    modified: String,
    #[serde(default)]
    expires: Option<String>,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    fields: Vec<DemoField>,
    #[serde(default)]
    attachments: Vec<DemoAttachment>,
}

#[derive(Deserialize)]
struct DemoField {
    key: String,
    value: Secret,
    protected: bool,
}

#[derive(Deserialize)]
struct DemoAttachment {
    name: String,
    size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub locked: bool,
    pub name: String,
    pub file: String,
    pub path: String,
}

pub struct DemoVault {
    data: DemoFile,
    locked: bool,
}

fn is_expired(expires: &Option<String>) -> bool {
    let now = Local::now().naive_local();
    expires
        .as_deref()
        .and_then(|e| NaiveDateTime::parse_from_str(e, ISO).ok())
        .is_some_and(|t| t <= now)
}

impl DemoVault {
    pub fn load() -> DemoVault {
        let data: DemoFile = serde_json::from_str(DEMO_JSON).expect("demo/vault.json is valid");
        DemoVault { data, locked: true }
    }

    pub fn status(&self) -> VaultStatus {
        VaultStatus {
            locked: self.locked,
            name: self.data.name.clone(),
            file: self.data.file.clone(),
            path: self.data.path.clone(),
        }
    }

    /// The demo accepts any non-empty password.
    pub fn unlock(&mut self, password: &Secret) -> AppResult<()> {
        if password.is_empty() {
            return Err(AppError::EmptyPassword);
        }
        self.locked = false;
        Ok(())
    }

    pub fn lock(&mut self) {
        self.locked = true;
    }

    fn open(&self) -> AppResult<&DemoFile> {
        if self.locked {
            Err(AppError::Locked)
        } else {
            Ok(&self.data)
        }
    }

    fn group_kind(g: &DemoGroup) -> GroupKind {
        match g.kind.as_str() {
            "root" => GroupKind::Root,
            "recycleBin" => GroupKind::RecycleBin,
            _ => GroupKind::Normal,
        }
    }

    fn in_recycle_bin(&self, e: &DemoEntry) -> bool {
        self.data
            .groups
            .iter()
            .any(|g| g.id == e.group && g.kind == "recycleBin")
    }

    fn depth<'a>(&'a self, mut id: &'a str) -> u32 {
        let mut depth = 0;
        while let Some(parent) = self
            .data
            .groups
            .iter()
            .find(|g| g.id == id)
            .and_then(|g| g.parent.as_deref())
        {
            depth += 1;
            id = parent;
        }
        depth
    }

    pub fn groups(&self) -> AppResult<Vec<GroupView>> {
        let data = self.open()?;
        Ok(data
            .groups
            .iter()
            .map(|g| GroupView {
                id: g.id.clone(),
                name: g.name.clone(),
                parent_id: g.parent.clone(),
                depth: self.depth(&g.id),
                kind: Self::group_kind(g),
                entry_count: self.entries_in(&g.id).count() as u32,
            })
            .collect())
    }

    /// The root group lists every entry outside the recycle bin; any other
    /// group lists its own entries. (What the root shows for a real database
    /// is decided in M2.)
    fn entries_in<'a>(&'a self, group_id: &'a str) -> impl Iterator<Item = &'a DemoEntry> + 'a {
        let root = self
            .data
            .groups
            .iter()
            .any(|g| g.id == group_id && g.kind == "root");
        self.data.entries.iter().filter(move |e| {
            if root {
                !self.in_recycle_bin(e)
            } else {
                e.group == group_id
            }
        })
    }

    fn summary(e: &DemoEntry) -> EntrySummary {
        EntrySummary {
            id: e.id.clone(),
            group_id: e.group.clone(),
            title: e.title.clone(),
            username: e.username.clone(),
            url: e.url.clone(),
            modified: e.modified.clone(),
            expired: is_expired(&e.expires),
            has_totp: e.otp,
            tags: e.tags.clone(),
        }
    }

    pub fn entries(&self, group_id: &str) -> AppResult<Vec<EntrySummary>> {
        self.open()?;
        let mut list: Vec<EntrySummary> = self.entries_in(group_id).map(Self::summary).collect();
        list.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(list)
    }

    /// Searches everything outside the recycle bin (features.md §3.4).
    pub fn search(&self, query: &str) -> AppResult<Vec<EntrySummary>> {
        let data = self.open()?;
        let mut hits: Vec<(u8, &DemoEntry)> = data
            .entries
            .iter()
            .filter(|e| !self.in_recycle_bin(e))
            .filter_map(|e| {
                let h = Haystack {
                    title: &e.title,
                    username: &e.username,
                    url: &e.url,
                    notes: &e.notes,
                    tags: &e.tags,
                };
                rank(query, &h).map(|r| (r, e))
            })
            .collect();
        hits.sort_by(|(ra, a), (rb, b)| ra.cmp(rb).then_with(|| b.modified.cmp(&a.modified)));
        Ok(hits.into_iter().map(|(_, e)| Self::summary(e)).collect())
    }

    fn entry_ref(&self, id: &str) -> AppResult<&DemoEntry> {
        self.open()?
            .entries
            .iter()
            .find(|e| e.id == id)
            .ok_or(AppError::NotFound)
    }

    pub fn entry(&self, id: &str) -> AppResult<EntryDetail> {
        let e = self.entry_ref(id)?;
        let mut group_path = Vec::new();
        let mut cur = Some(e.group.as_str());
        while let Some(gid) = cur {
            let g = self
                .data
                .groups
                .iter()
                .find(|g| g.id == gid)
                .ok_or(AppError::NotFound)?;
            group_path.push(g.name.clone());
            cur = g.parent.as_deref();
        }
        group_path.reverse();
        Ok(EntryDetail {
            id: e.id.clone(),
            group_id: e.group.clone(),
            group_path,
            title: e.title.clone(),
            username: e.username.clone(),
            url: e.url.clone(),
            notes: e.notes.clone(),
            tags: e.tags.clone(),
            created: e.created.clone(),
            modified: e.modified.clone(),
            expires: e.expires.clone(),
            expired: is_expired(&e.expires),
            has_password: !e.password.is_empty(),
            has_totp: e.otp,
            fields: e
                .fields
                .iter()
                .map(|f| FieldView {
                    key: f.key.clone(),
                    protected: f.protected,
                    value: (!f.protected).then(|| f.value.expose().to_string()),
                })
                .collect(),
            attachments: e
                .attachments
                .iter()
                .map(|a| AttachmentView {
                    name: a.name.clone(),
                    size: a.size,
                })
                .collect(),
        })
    }

    /// One field's value. Standard keys follow KeePass (`UserName`,
    /// `Password`, ...); anything else is a custom field name.
    pub fn field(&self, id: &str, key: &str) -> AppResult<Secret> {
        let e = self.entry_ref(id)?;
        Ok(match key {
            fields::TITLE => Secret::new(e.title.clone()),
            fields::USERNAME => Secret::new(e.username.clone()),
            fields::PASSWORD => e.password.clone(),
            fields::URL => Secret::new(e.url.clone()),
            fields::NOTES => Secret::new(e.notes.clone()),
            fields::TOTP => Secret::new(self.totp(id)?.code),
            custom => e
                .fields
                .iter()
                .find(|f| f.key == custom)
                .map(|f| f.value.clone())
                .ok_or(AppError::NotFound)?,
        })
    }

    /// Demo codes only: stable per entry and 30 s window, not real TOTP.
    /// Real TOTP (RFC 6238, otpauth URIs, KeePass 2 fields) arrives in M3.
    pub fn totp(&self, id: &str) -> AppResult<TotpView> {
        let e = self.entry_ref(id)?;
        if !e.otp {
            return Err(AppError::NotFound);
        }
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        let step = now / 30;
        let mut h: u32 = 2_166_136_261;
        for b in format!("{}{}", e.id, step).bytes() {
            h = (h ^ u32::from(b)).wrapping_mul(16_777_619);
        }
        Ok(TotpView {
            code: format!("{:06}", h % 1_000_000),
            period: 30,
            remaining: (30 - now % 30) as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unlocked() -> DemoVault {
        let mut v = DemoVault::load();
        v.unlock(&Secret::new("x")).unwrap();
        v
    }

    #[test]
    fn locked_vault_refuses_everything() {
        let v = DemoVault::load();
        assert!(matches!(v.groups(), Err(AppError::Locked)));
        assert!(matches!(v.entry("github"), Err(AppError::Locked)));
        assert!(matches!(
            v.field("github", fields::PASSWORD),
            Err(AppError::Locked)
        ));
    }

    #[test]
    fn empty_password_is_rejected() {
        let mut v = DemoVault::load();
        assert!(matches!(
            v.unlock(&Secret::default()),
            Err(AppError::EmptyPassword)
        ));
    }

    #[test]
    fn views_never_contain_protected_values() {
        let v = unlocked();
        for g in v.groups().unwrap() {
            for s in v.entries(&g.id).unwrap() {
                let detail = v.entry(&s.id).unwrap();
                let json =
                    serde_json::to_string(&detail).unwrap() + &serde_json::to_string(&s).unwrap();
                let e = v.entry_ref(&s.id).unwrap();
                assert!(
                    !json.contains(e.password.expose()),
                    "password of {} leaked",
                    s.id
                );
                for f in e.fields.iter().filter(|f| f.protected) {
                    assert!(
                        !json.contains(f.value.expose()),
                        "protected field of {} leaked",
                        s.id
                    );
                }
            }
        }
    }

    #[test]
    fn search_skips_recycle_bin_and_ranks_titles_first() {
        let v = unlocked();
        let hits = v.search("git").unwrap();
        assert_eq!(hits.first().map(|h| h.title.as_str()), Some("GitHub"));
        assert!(v.search("oldcorp").unwrap().is_empty());
    }

    #[test]
    fn expired_entries_are_marked() {
        let v = unlocked();
        assert!(v.entry("vpn").unwrap().expired);
        assert!(!v.entry("github").unwrap().expired);
    }
}
