//! What the frontend is allowed to see of a database.
//!
//! These types are the whole contract between the Rust side and the UI (see
//! docs/architecture.md). None of them carries a password, a protected field
//! value or a TOTP secret: those stay in Rust and are only handed out one at
//! a time, on request, by id.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupKind {
    Root,
    Normal,
    RecycleBin,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub depth: u32,
    pub kind: GroupKind,
    pub entry_count: u32,
}

/// One row of the entry list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub username: String,
    pub url: String,
    /// ISO 8601, local time, e.g. `2026-11-12T14:20:00`
    pub modified: String,
    pub expired: bool,
    pub has_totp: bool,
    pub tags: Vec<String>,
}

/// A custom field. `value` is `None` when the field is protected: the
/// frontend asks for it with `reveal_field` only when the user wants to see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldView {
    pub key: String,
    pub protected: bool,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentView {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDetail {
    pub id: String,
    pub group_id: String,
    /// group names from the root down to the entry's group
    pub group_path: Vec<String>,
    pub title: String,
    pub username: String,
    pub url: String,
    pub notes: String,
    pub tags: Vec<String>,
    pub created: String,
    pub modified: String,
    /// `None` means "never expires"
    pub expires: Option<String>,
    pub expired: bool,
    pub has_password: bool,
    pub has_totp: bool,
    pub fields: Vec<FieldView>,
    pub attachments: Vec<AttachmentView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpView {
    pub code: String,
    pub period: u32,
    /// seconds until the code changes
    pub remaining: u32,
}

/// Field keys as KeePass stores them.
pub mod fields {
    pub const TITLE: &str = "Title";
    pub const USERNAME: &str = "UserName";
    pub const PASSWORD: &str = "Password";
    pub const URL: &str = "URL";
    pub const NOTES: &str = "Notes";
    /// pseudo-field: the current TOTP code, computed on the Rust side
    pub const TOTP: &str = "@totp";
}
