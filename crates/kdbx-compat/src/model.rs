//! Canonical form of a database as keepass-rs understands it, using the same
//! paths as [`crate::canon`], so that keepass-rs' view of a file can be
//! compared with keepassxc-cli's export of the same file.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use keepass::Database;
use keepass::db::{
    CustomDataItem, CustomDataValue, DataTransferObfuscation, EntryRef, GroupRef, Icon, Times,
};

use crate::canon::{Canon, EXISTS, format_time, short_hash};

fn b64_uuid(bytes: &[u8; 16]) -> String {
    B64.encode(bytes)
}

fn bool_str(b: bool) -> String {
    if b { "True".into() } else { "False".into() }
}

struct Out<'a> {
    canon: &'a mut Canon,
}

impl Out<'_> {
    fn put(&mut self, path: &str, value: impl Into<String>) {
        let value = value.into();
        if value.is_empty() {
            return;
        }
        self.canon.records.insert(path.to_string(), value);
    }

    fn opt(&mut self, path: &str, value: Option<impl ToString>) {
        if let Some(v) = value {
            self.put(path, v.to_string());
        }
    }

    fn times(&mut self, prefix: &str, t: &Times) {
        let p = |n: &str| format!("{prefix}/Times/{n}");
        self.opt(&p("CreationTime"), t.creation.as_ref().map(format_time));
        self.opt(
            &p("LastModificationTime"),
            t.last_modification.as_ref().map(format_time),
        );
        self.opt(
            &p("LastAccessTime"),
            t.last_access.as_ref().map(format_time),
        );
        self.put(&p("ExpiryTime"), format_time(&t.expiry));
        self.opt(&p("Expires"), t.expires.map(bool_str));
        self.opt(&p("UsageCount"), t.usage_count);
        self.opt(
            &p("LocationChanged"),
            t.location_changed.as_ref().map(format_time),
        );
    }

    fn custom_data<'a>(
        &mut self,
        prefix: &str,
        items: impl Iterator<Item = (&'a String, &'a CustomDataItem)>,
    ) {
        for (key, item) in items {
            let p = format!("{prefix}/CustomData/Item[{key}]");
            self.put(&p, EXISTS);
            let value = match &item.value {
                Some(CustomDataValue::String(s)) => s.clone(),
                Some(CustomDataValue::Binary(b)) => B64.encode(b),
                None => String::new(),
            };
            self.put(&format!("{p}/Value"), value);
            self.opt(
                &format!("{p}/LastModificationTime"),
                item.last_modification_time.as_ref().map(format_time),
            );
        }
    }

    fn icon(&mut self, prefix: &str, icon: Option<&Icon>) {
        match icon {
            Some(Icon::BuiltIn(i)) => self.put(&format!("{prefix}/IconID"), i.to_string()),
            Some(Icon::Custom(id)) => self.put(
                &format!("{prefix}/CustomIconUUID"),
                b64_uuid(id.uuid().as_bytes()),
            ),
            None => {}
        }
    }

    fn tags(&mut self, path: &str, tags: &[String]) {
        let tags: Vec<&str> = tags
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect();
        if !tags.is_empty() {
            self.put(path, tags.join(";"));
        }
    }
}

pub fn from_database(db: &Database) -> Canon {
    let mut canon = Canon::default();
    let mut out = Out { canon: &mut canon };
    meta(&mut out, db);
    let root = db.root();
    group(&mut out, "Root", &root);

    for (uuid, when) in &db.deleted_objects {
        let p = format!(
            "Root/DeletedObjects/DeletedObject[{}]",
            b64_uuid(uuid.as_bytes())
        );
        out.put(&p, EXISTS);
        out.opt(&format!("{p}/DeletionTime"), when.as_ref().map(format_time));
    }
    canon
}

fn meta(out: &mut Out<'_>, db: &Database) {
    let m = &db.meta;
    let p = |n: &str| format!("Meta/{n}");
    out.opt(&p("DatabaseName"), m.database_name.as_ref());
    out.opt(
        &p("DatabaseNameChanged"),
        m.database_name_changed.as_ref().map(format_time),
    );
    out.opt(&p("DatabaseDescription"), m.database_description.as_ref());
    out.opt(
        &p("DatabaseDescriptionChanged"),
        m.database_description_changed.as_ref().map(format_time),
    );
    out.opt(&p("DefaultUserName"), m.default_username.as_ref());
    out.opt(
        &p("DefaultUserNameChanged"),
        m.default_username_changed.as_ref().map(format_time),
    );
    out.opt(&p("MaintenanceHistoryDays"), m.maintenance_history_days);
    out.opt(
        &p("Color"),
        m.color.as_ref().map(|c| c.to_string().to_ascii_uppercase()),
    );
    out.opt(
        &p("MasterKeyChanged"),
        m.master_key_changed.as_ref().map(format_time),
    );
    out.opt(&p("MasterKeyChangeRec"), m.master_key_change_rec);
    out.opt(&p("MasterKeyChangeForce"), m.master_key_change_force);
    if let Some(mp) = &m.memory_protection {
        out.put(
            &p("MemoryProtection/ProtectTitle"),
            bool_str(mp.protect_title),
        );
        out.put(
            &p("MemoryProtection/ProtectUserName"),
            bool_str(mp.protect_username),
        );
        out.put(
            &p("MemoryProtection/ProtectPassword"),
            bool_str(mp.protect_password),
        );
        out.put(&p("MemoryProtection/ProtectURL"), bool_str(mp.protect_url));
        out.put(
            &p("MemoryProtection/ProtectNotes"),
            bool_str(mp.protect_notes),
        );
    }
    for icon in db.iter_all_custom_icons() {
        let ip = p(&format!(
            "CustomIcons/Icon[{}]",
            b64_uuid(icon.id().uuid().as_bytes())
        ));
        out.put(&ip, EXISTS);
        out.put(
            &format!("{ip}/Data"),
            format!(
                "{} bytes, {}",
                icon.data.len(),
                short_hash(B64.encode(&icon.data).as_bytes())
            ),
        );
        out.opt(&format!("{ip}/Name"), icon.name.as_ref());
        out.opt(
            &format!("{ip}/LastModificationTime"),
            icon.last_modification_time.as_ref().map(format_time),
        );
    }
    out.opt(&p("RecycleBinEnabled"), m.recyclebin_enabled.map(bool_str));
    out.opt(
        &p("RecycleBinUUID"),
        m.recyclebin_uuid
            .filter(|u| !u.is_nil())
            .map(|u| b64_uuid(u.as_bytes())),
    );
    out.opt(
        &p("RecycleBinChanged"),
        m.recyclebin_changed.as_ref().map(format_time),
    );
    out.opt(
        &p("EntryTemplatesGroup"),
        m.entry_templates_group
            .filter(|u| !u.is_nil())
            .map(|u| b64_uuid(u.as_bytes())),
    );
    out.opt(
        &p("EntryTemplatesGroupChanged"),
        m.entry_templates_group_changed.as_ref().map(format_time),
    );
    out.opt(
        &p("LastSelectedGroup"),
        m.last_selected_group
            .filter(|u| !u.is_nil())
            .map(|u| b64_uuid(u.as_bytes())),
    );
    out.opt(
        &p("LastTopVisibleGroup"),
        m.last_top_visible_group
            .filter(|u| !u.is_nil())
            .map(|u| b64_uuid(u.as_bytes())),
    );
    out.opt(&p("HistoryMaxItems"), m.history_max_items);
    out.opt(&p("HistoryMaxSize"), m.history_max_size);
    out.opt(
        &p("SettingsChanged"),
        m.settings_changed.as_ref().map(format_time),
    );
    out.custom_data("Meta", m.custom_data.iter());
}

fn group(out: &mut Out<'_>, prefix: &str, g: &GroupRef<'_>) {
    let seg = format!("Group[{}]", b64_uuid(g.id().uuid().as_bytes()));
    let path = format!("{prefix}/{seg}");
    out.put(&path, EXISTS);
    out.canon
        .labels
        .entry(seg)
        .or_insert_with(|| format!("group \"{}\"", g.name));

    out.put(&format!("{path}/Name"), g.name.clone());
    out.opt(&format!("{path}/Notes"), g.notes.as_ref());
    out.tags(&format!("{path}/Tags"), &g.tags);
    out.icon(&path, g.icon());
    out.times(&path, &g.times);
    out.put(&format!("{path}/IsExpanded"), bool_str(g.is_expanded));
    out.opt(
        &format!("{path}/DefaultAutoTypeSequence"),
        g.default_autotype_sequence.as_ref(),
    );
    out.opt(
        &format!("{path}/EnableAutoType"),
        g.enable_autotype.map(bool_str),
    );
    out.opt(
        &format!("{path}/EnableSearching"),
        g.enable_searching.map(bool_str),
    );
    out.opt(
        &format!("{path}/PreviousParentGroup"),
        g.previous_parent()
            .map(|p| b64_uuid(p.id().uuid().as_bytes())),
    );
    out.custom_data(&path, g.custom_data.iter());

    let groups: Vec<String> = g
        .groups()
        .map(|c| format!("Group[{}]", b64_uuid(c.id().uuid().as_bytes())))
        .collect();
    let entries: Vec<String> = g
        .entries()
        .map(|e| format!("Entry[{}]", b64_uuid(e.id().uuid().as_bytes())))
        .collect();
    if !groups.is_empty() {
        out.put(&format!("{path}/@groups"), groups.join(" "));
    }
    if !entries.is_empty() {
        out.put(&format!("{path}/@entries"), entries.join(" "));
    }

    for child in g.groups() {
        group(out, &path, &child);
    }
    for e in g.entries() {
        let seg = format!("Entry[{}]", b64_uuid(e.id().uuid().as_bytes()));
        out.canon
            .labels
            .entry(seg.clone())
            .or_insert_with(|| format!("entry \"{}\"", e.get_title().unwrap_or("")));
        entry(out, &format!("{path}/{seg}"), &e, true);
    }
}

fn entry(out: &mut Out<'_>, path: &str, e: &EntryRef<'_>, current: bool) {
    out.put(path, EXISTS);
    if !current {
        out.put(&format!("{path}/UUID"), b64_uuid(e.id().uuid().as_bytes()));
    }
    out.icon(path, e.icon());
    out.opt(
        &format!("{path}/ForegroundColor"),
        e.foreground_color
            .as_ref()
            .map(|c| c.to_string().to_ascii_uppercase()),
    );
    out.opt(
        &format!("{path}/BackgroundColor"),
        e.background_color
            .as_ref()
            .map(|c| c.to_string().to_ascii_uppercase()),
    );
    out.opt(&format!("{path}/OverrideURL"), e.override_url.as_ref());
    out.tags(&format!("{path}/Tags"), &e.tags);
    out.times(path, &e.times);
    out.put(&format!("{path}/QualityCheck"), bool_str(e.quality_check));
    out.opt(
        &format!("{path}/PreviousParentGroup"),
        e.previous_parent()
            .map(|p| b64_uuid(p.id().uuid().as_bytes())),
    );

    for (key, value) in &e.fields {
        let p = format!("{path}/String[{key}]");
        out.put(&p, EXISTS);
        out.put(&format!("{p}/Value"), value.get().clone());
        if value.is_protected() {
            out.put(&format!("{p}/Value@Protected"), "True");
        }
    }
    for (name, _) in e.attachments_named() {
        let p = format!("{path}/Binary[{name}]");
        out.put(&p, EXISTS);
        out.put(&format!("{p}/Value@Ref"), EXISTS);
    }
    if let Some(at) = &e.autotype {
        let p = format!("{path}/AutoType");
        out.put(&format!("{p}/Enabled"), bool_str(at.enabled));
        let obf = match at.data_transfer_obfuscation {
            DataTransferObfuscation::None => "0",
            DataTransferObfuscation::UseClipboard => "1",
        };
        out.put(&format!("{p}/DataTransferObfuscation"), obf);
        out.opt(
            &format!("{p}/DefaultSequence"),
            at.default_sequence.as_ref(),
        );
        for (i, a) in at.associations.iter().enumerate() {
            let ap = format!("{p}/Association#{i}");
            out.put(&ap, EXISTS);
            out.put(&format!("{ap}/Window"), a.window.clone());
            out.put(&format!("{ap}/KeystrokeSequence"), a.sequence.clone());
        }
    }
    out.custom_data(path, e.custom_data.iter());

    if current && let Some(history) = &e.history {
        for i in 0..history.get_entries().len() {
            if let Some(h) = e.historical(i) {
                entry(out, &format!("{path}/History/Entry#{i}"), &h, false);
            }
        }
    }
}

/// Attachment contents per entry, keyed like the canonical paths:
/// `.../Entry[uuid]/Binary[name]` and `.../Entry[uuid]/History/Entry#i/Binary[name]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentInfo {
    pub len: usize,
    pub hash: String,
    pub protected: bool,
}

pub fn attachments(db: &Database) -> BTreeMap<String, AttachmentInfo> {
    let mut out = BTreeMap::new();
    fn walk(out: &mut BTreeMap<String, AttachmentInfo>, prefix: &str, g: &GroupRef<'_>) {
        let path = format!("{prefix}/Group[{}]", b64_uuid(g.id().uuid().as_bytes()));
        for child in g.groups() {
            walk(out, &path, &child);
        }
        for e in g.entries() {
            let ep = format!("{path}/Entry[{}]", b64_uuid(e.id().uuid().as_bytes()));
            collect(out, &ep, &e);
            if let Some(history) = &e.history {
                for i in 0..history.get_entries().len() {
                    if let Some(h) = e.historical(i) {
                        collect(out, &format!("{ep}/History/Entry#{i}"), &h);
                    }
                }
            }
        }
    }
    fn collect(out: &mut BTreeMap<String, AttachmentInfo>, path: &str, e: &EntryRef<'_>) {
        for (name, att) in e.attachments_named() {
            let data = att.data.get();
            out.insert(
                format!("{path}/Binary[{name}]"),
                AttachmentInfo {
                    len: data.len(),
                    hash: short_hash(data),
                    protected: att.data.is_protected(),
                },
            );
        }
    }
    walk(&mut out, "Root", &db.root());
    out
}

/// keepassxc-cli addresses entries by "Group/Sub/Title" below the root group.
pub fn cli_entry_paths(db: &Database) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    fn walk(
        out: &mut BTreeMap<String, String>,
        canon_prefix: &str,
        cli_prefix: &str,
        g: &GroupRef<'_>,
        is_root: bool,
    ) {
        let path = format!(
            "{canon_prefix}/Group[{}]",
            b64_uuid(g.id().uuid().as_bytes())
        );
        let cli = if is_root {
            String::new()
        } else if cli_prefix.is_empty() {
            g.name.clone()
        } else {
            format!("{cli_prefix}/{}", g.name)
        };
        for child in g.groups() {
            walk(out, &path, &cli, &child, false);
        }
        for e in g.entries() {
            let title = e.get_title().unwrap_or("");
            let cli_path = if cli.is_empty() {
                title.to_string()
            } else {
                format!("{cli}/{title}")
            };
            out.insert(
                format!("{path}/Entry[{}]", b64_uuid(e.id().uuid().as_bytes())),
                cli_path,
            );
        }
    }
    walk(&mut out, "Root", "", &db.root(), true);
    out
}
