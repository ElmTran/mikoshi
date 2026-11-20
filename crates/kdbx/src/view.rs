//! Typed, read-only views on the XML tree (groups, entries, fields, meta)
//! and the few edits Mikoshi makes. Edits touch only the nodes they are
//! about; the rest of the document stays as it was read.

use std::borrow::Cow;
use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

use crate::db::Database;
use crate::header::Binary;
use crate::types::{Timestamp, Uuid};
use crate::xml::{Element, Node};

/// Where each current group and entry sits in the tree: child indices from
/// the document root. Rebuilt after every structural edit.
#[derive(Default)]
pub(crate) struct Index {
    pub(crate) root_group: Option<Vec<usize>>,
    pub(crate) groups: HashMap<Uuid, Vec<usize>>,
    pub(crate) entries: HashMap<Uuid, Vec<usize>>,
}

pub(crate) fn at<'a>(root: &'a Element, path: &[usize]) -> Option<&'a Element> {
    let mut el = root;
    for &i in path {
        el = match el.children.get(i)? {
            Node::Element(e) => e,
            _ => return None,
        };
    }
    Some(el)
}

pub(crate) fn at_mut<'a>(root: &'a mut Element, path: &[usize]) -> Option<&'a mut Element> {
    let mut el = root;
    for &i in path {
        el = match el.children.get_mut(i)? {
            Node::Element(e) => e,
            _ => return None,
        };
    }
    Some(el)
}

pub(crate) fn uuid_of(el: &Element) -> Option<Uuid> {
    Uuid::from_base64(&el.child_text("UUID")?)
}

pub(crate) fn opt_bool(el: &Element, name: &str) -> Option<bool> {
    let t = el.child_text(name)?;
    let t = t.trim();
    if t.eq_ignore_ascii_case("true") {
        Some(true)
    } else if t.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

fn non_empty(c: Option<Cow<'_, str>>) -> Option<Cow<'_, str>> {
    c.filter(|s| !s.trim().is_empty())
}

fn tags_of(el: &Element) -> Vec<String> {
    el.child_text("Tags")
        .map(|t| {
            t.split([';', ','])
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn icon_of(el: &Element) -> Option<u32> {
    el.child_text("IconID")?.trim().parse().ok()
}

fn custom_icon_of(el: &Element) -> Option<Uuid> {
    uuid_of_text(el, "CustomIconUUID")
}

pub(crate) fn uuid_of_text(el: &Element, name: &str) -> Option<Uuid> {
    Uuid::from_base64(&el.child_text(name)?).filter(|u| !u.is_nil())
}

impl Database {
    pub(crate) fn reindex(&mut self) {
        let mut idx = Index::default();
        fn walk(g: &Element, path: &mut Vec<usize>, idx: &mut Index) {
            for (i, c) in g.children.iter().enumerate() {
                let Node::Element(e) = c else { continue };
                path.push(i);
                match e.name.as_str() {
                    "Group" => {
                        if let Some(u) = uuid_of(e) {
                            idx.groups.entry(u).or_insert_with(|| path.clone());
                        }
                        walk(e, path, idx);
                    }
                    "Entry" => {
                        if let Some(u) = uuid_of(e) {
                            idx.entries.entry(u).or_insert_with(|| path.clone());
                        }
                    }
                    _ => {}
                }
                path.pop();
            }
        }
        let root_pos = self
            .root
            .children
            .iter()
            .position(|c| matches!(c, Node::Element(e) if e.name == "Root"));
        if let Some(r) = root_pos
            && let Node::Element(root) = &self.root.children[r]
            && let Some(g) = root
                .children
                .iter()
                .position(|c| matches!(c, Node::Element(e) if e.name == "Group"))
            && let Node::Element(group) = &root.children[g]
        {
            let mut path = vec![r, g];
            if let Some(u) = uuid_of(group) {
                idx.groups.insert(u, path.clone());
            }
            idx.root_group = Some(path.clone());
            walk(group, &mut path, &mut idx);
        }
        self.index = idx;
    }

    pub fn meta(&self) -> Meta<'_> {
        static EMPTY: Element = Element {
            name: String::new(),
            attrs: Vec::new(),
            children: Vec::new(),
        };
        Meta(self.root.child("Meta").unwrap_or(&EMPTY))
    }

    pub fn root_group(&self) -> Option<Group<'_>> {
        let el = at(&self.root, self.index.root_group.as_deref()?)?;
        Some(Group { db: self, el })
    }

    pub fn group(&self, id: Uuid) -> Option<Group<'_>> {
        let el = at(&self.root, self.index.groups.get(&id)?)?;
        Some(Group { db: self, el })
    }

    pub fn entry(&self, id: Uuid) -> Option<Entry<'_>> {
        let el = at(&self.root, self.index.entries.get(&id)?)?;
        Some(Entry { db: self, el })
    }

    /// The group an entry is in.
    pub fn parent_of_entry(&self, id: Uuid) -> Option<Group<'_>> {
        let path = self.index.entries.get(&id)?;
        let el = at(&self.root, &path[..path.len() - 1])?;
        Some(Group { db: self, el })
    }

    /// The recycle bin, when it is turned on and exists.
    pub fn recycle_bin(&self) -> Option<Group<'_>> {
        let m = self.meta();
        if !m.recycle_bin_enabled() {
            return None;
        }
        self.group(m.recycle_bin()?)
    }

    /// Every current entry (no history), depth first in file order.
    pub fn entries(&self) -> Vec<Entry<'_>> {
        let mut out = Vec::new();
        fn walk<'a>(g: Group<'a>, out: &mut Vec<Entry<'a>>) {
            out.extend(g.entries());
            for c in g.groups() {
                walk(c, out);
            }
        }
        if let Some(r) = self.root_group() {
            walk(r, &mut out);
        }
        out
    }

    /// Every group, the root included, depth first in file order.
    pub fn groups(&self) -> Vec<Group<'_>> {
        let mut out = Vec::new();
        fn walk<'a>(g: Group<'a>, out: &mut Vec<Group<'a>>) {
            out.push(g);
            for c in g.groups() {
                walk(c, out);
            }
        }
        if let Some(r) = self.root_group() {
            walk(r, &mut out);
        }
        out
    }

    fn binary(&self, el: &Element) -> Option<&Binary> {
        let r: usize = el.attr("Ref")?.trim().parse().ok()?;
        self.binaries.get(r)
    }
}

// ---------------------------------------------------------------- Meta

#[derive(Clone, Copy)]
pub struct Meta<'a>(&'a Element);

pub struct CustomIcon<'a> {
    pub uuid: Uuid,
    pub data: Vec<u8>,
    pub name: Option<Cow<'a, str>>,
}

impl<'a> Meta<'a> {
    pub fn element(&self) -> &'a Element {
        self.0
    }

    pub fn database_name(&self) -> Cow<'a, str> {
        self.0.child_text("DatabaseName").unwrap_or_default()
    }

    /// `RecycleBinEnabled`, true when missing (as in KeePass).
    pub fn recycle_bin_enabled(&self) -> bool {
        opt_bool(self.0, "RecycleBinEnabled").unwrap_or(true)
    }

    pub fn recycle_bin(&self) -> Option<Uuid> {
        uuid_of_text(self.0, "RecycleBinUUID")
    }

    /// `HistoryMaxItems`: `None` means no limit (a negative value). KeePass's
    /// default, 10, when missing or unreadable.
    pub fn history_max_items(&self) -> Option<usize> {
        let n: i64 = self
            .0
            .child_text("HistoryMaxItems")
            .and_then(|t| t.trim().parse().ok())
            .unwrap_or(10);
        usize::try_from(n).ok()
    }

    /// `HistoryMaxSize` in bytes: `None` means no limit (a negative value).
    /// KeePass's default, 6 MiB, when missing or unreadable.
    pub fn history_max_size(&self) -> Option<u64> {
        let n: i64 = self
            .0
            .child_text("HistoryMaxSize")
            .and_then(|t| t.trim().parse().ok())
            .unwrap_or(6 * 1024 * 1024);
        u64::try_from(n).ok()
    }

    pub fn custom_icons(&self) -> Vec<CustomIcon<'a>> {
        let Some(icons) = self.0.child("CustomIcons") else {
            return Vec::new();
        };
        icons
            .children_named("Icon")
            .filter_map(|i| {
                let uuid = uuid_of(i)?;
                let text = i.child_text("Data")?;
                let clean: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
                Some(CustomIcon {
                    uuid,
                    data: B64.decode(clean).ok()?,
                    name: non_empty(i.child_text("Name")),
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------- Times

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Times {
    pub creation: Option<Timestamp>,
    pub last_modification: Option<Timestamp>,
    pub last_access: Option<Timestamp>,
    pub expiry: Option<Timestamp>,
    pub expires: bool,
    pub usage_count: Option<u64>,
    pub location_changed: Option<Timestamp>,
}

impl Times {
    /// Expiry time, when the item expires at all.
    pub fn expires_at(&self) -> Option<Timestamp> {
        self.expiry.filter(|_| self.expires)
    }

    pub fn expired(&self) -> bool {
        self.expires_at().is_some_and(|t| t <= Timestamp::now())
    }
}

fn times_of(el: &Element) -> Times {
    let Some(t) = el.child("Times") else {
        return Times::default();
    };
    let time = |n: &str| t.child_text(n).and_then(|s| Timestamp::parse(&s));
    Times {
        creation: time("CreationTime"),
        last_modification: time("LastModificationTime"),
        last_access: time("LastAccessTime"),
        expiry: time("ExpiryTime"),
        expires: opt_bool(t, "Expires").unwrap_or(false),
        usage_count: t
            .child_text("UsageCount")
            .and_then(|s| s.trim().parse().ok()),
        location_changed: time("LocationChanged"),
    }
}

// ---------------------------------------------------------------- Group

#[derive(Clone, Copy)]
pub struct Group<'a> {
    db: &'a Database,
    el: &'a Element,
}

impl<'a> Group<'a> {
    pub fn element(&self) -> &'a Element {
        self.el
    }
    pub fn uuid(&self) -> Option<Uuid> {
        uuid_of(self.el)
    }
    pub fn name(&self) -> Cow<'a, str> {
        self.el.child_text("Name").unwrap_or_default()
    }
    pub fn notes(&self) -> Cow<'a, str> {
        self.el.child_text("Notes").unwrap_or_default()
    }
    pub fn icon_id(&self) -> Option<u32> {
        icon_of(self.el)
    }
    pub fn custom_icon(&self) -> Option<Uuid> {
        custom_icon_of(self.el)
    }
    pub fn tags(&self) -> Vec<String> {
        tags_of(self.el)
    }
    pub fn times(&self) -> Times {
        times_of(self.el)
    }
    /// `IsExpanded`, true when missing.
    pub fn is_expanded(&self) -> bool {
        opt_bool(self.el, "IsExpanded").unwrap_or(true)
    }
    /// `EnableSearching`; `None` ("null" or missing) means inherit.
    pub fn enable_searching(&self) -> Option<bool> {
        opt_bool(self.el, "EnableSearching")
    }
    /// `EnableAutoType`; `None` means inherit.
    pub fn enable_autotype(&self) -> Option<bool> {
        opt_bool(self.el, "EnableAutoType")
    }
    pub fn default_autotype_sequence(&self) -> Option<Cow<'a, str>> {
        non_empty(self.el.child_text("DefaultAutoTypeSequence"))
    }
    pub fn groups(&self) -> impl Iterator<Item = Group<'a>> + use<'a> {
        let db = self.db;
        self.el
            .children_named("Group")
            .map(move |el| Group { db, el })
    }
    pub fn entries(&self) -> impl Iterator<Item = Entry<'a>> + use<'a> {
        let db = self.db;
        self.el
            .children_named("Entry")
            .map(move |el| Entry { db, el })
    }
}

// ---------------------------------------------------------------- Entry

#[derive(Clone, Copy)]
pub struct Entry<'a> {
    db: &'a Database,
    el: &'a Element,
}

#[derive(Clone, Copy)]
pub struct Field<'a> {
    el: &'a Element,
}

impl<'a> Field<'a> {
    pub fn key(&self) -> Cow<'a, str> {
        self.el.child_text("Key").unwrap_or_default()
    }
    pub fn value(&self) -> Cow<'a, str> {
        match self.el.child("Value") {
            Some(v) => match v.secret() {
                Some(s) => String::from_utf8_lossy(s),
                None => v.text(),
            },
            None => Cow::Borrowed(""),
        }
    }
    /// Encrypted in the file (`Protected="True"`).
    pub fn protected(&self) -> bool {
        self.el.child("Value").is_some_and(|v| v.secret().is_some())
    }
}

pub struct Attachment<'a> {
    pub name: Cow<'a, str>,
    /// `None` when it points to a pool item that does not exist
    pub data: Option<Cow<'a, [u8]>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Association {
    pub window: String,
    pub sequence: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoType {
    /// `Enabled`, true when missing
    pub enabled: bool,
    pub default_sequence: Option<String>,
    pub obfuscation: u32,
    pub associations: Vec<Association>,
}

impl<'a> Entry<'a> {
    pub fn element(&self) -> &'a Element {
        self.el
    }
    pub fn uuid(&self) -> Option<Uuid> {
        uuid_of(self.el)
    }
    pub fn icon_id(&self) -> Option<u32> {
        icon_of(self.el)
    }
    pub fn custom_icon(&self) -> Option<Uuid> {
        custom_icon_of(self.el)
    }
    pub fn tags(&self) -> Vec<String> {
        tags_of(self.el)
    }
    pub fn times(&self) -> Times {
        times_of(self.el)
    }

    pub fn fields(&self) -> impl Iterator<Item = Field<'a>> + use<'a> {
        self.el.children_named("String").map(|el| Field { el })
    }

    pub fn field(&self, key: &str) -> Option<Field<'a>> {
        self.fields().find(|f| f.key() == key)
    }

    /// A field's value, protected or not.
    pub fn get(&self, key: &str) -> Option<Cow<'a, str>> {
        self.field(key).map(|f| f.value())
    }

    pub fn attachments(&self) -> impl Iterator<Item = Attachment<'a>> + use<'a> {
        let db = self.db;
        self.el.children_named("Binary").map(move |b| {
            let name = b.child_text("Key").unwrap_or_default();
            let data = b.child("Value").and_then(|v| {
                if v.attr("Ref").is_some() {
                    return db.binary(v).map(|bin| Cow::Borrowed(&bin.data[..]));
                }
                match v.secret() {
                    Some(s) => Some(Cow::Borrowed(s)),
                    None => {
                        let clean: String = v
                            .text()
                            .chars()
                            .filter(|c| !c.is_ascii_whitespace())
                            .collect();
                        B64.decode(clean).ok().map(Cow::Owned)
                    }
                }
            });
            Attachment { name, data }
        })
    }

    /// `None` when the entry has no `AutoType` element.
    pub fn autotype(&self) -> Option<AutoType> {
        let a = self.el.child("AutoType")?;
        Some(AutoType {
            enabled: opt_bool(a, "Enabled").unwrap_or(true),
            default_sequence: non_empty(a.child_text("DefaultSequence")).map(Cow::into_owned),
            obfuscation: a
                .child_text("DataTransferObfuscation")
                .and_then(|t| t.trim().parse().ok())
                .unwrap_or(0),
            associations: a
                .children_named("Association")
                .map(|x| Association {
                    window: x.child_text("Window").unwrap_or_default().into_owned(),
                    sequence: x
                        .child_text("KeystrokeSequence")
                        .unwrap_or_default()
                        .into_owned(),
                })
                .collect(),
        })
    }

    /// Roughly how much memory the entry takes, the way KeePass counts it
    /// (`PwEntry.GetSize`) to enforce `HistoryMaxSize`. History not included.
    pub fn approx_size(&self) -> u64 {
        let len = |c: Option<Cow<'_, str>>| c.map_or(0, |t| t.len() as u64);
        let mut n = 128;
        for f in self.fields() {
            n += f.key().len() as u64 + f.value().len() as u64;
        }
        for a in self.attachments() {
            n += a.name.len() as u64 + a.data.map_or(0, |d| d.len() as u64);
        }
        n += len(self.el.child_text("Tags"));
        n += len(self.el.child_text("OverrideURL"));
        if let Some(a) = self.autotype() {
            n += a.default_sequence.map_or(0, |s| s.len() as u64);
            for x in a.associations {
                n += (x.window.len() + x.sequence.len()) as u64;
            }
        }
        n
    }

    /// Older versions, oldest first.
    pub fn history(&self) -> impl Iterator<Item = Entry<'a>> + use<'a> {
        let db = self.db;
        self.el
            .child("History")
            .into_iter()
            .flat_map(|h| h.children_named("Entry"))
            .map(move |el| Entry { db, el })
    }
}

// ---------------------------------------------------------------- editing

pub(crate) fn now_text() -> String {
    Timestamp::now().to_base64()
}

pub(crate) fn set_child_text(el: &mut Element, name: &str, text: &str, after: &[&str]) {
    match el.child_mut(name) {
        Some(c) => c.set_text(text),
        None => el.insert_element(Element::with_text(name, text), after, &[]),
    }
}

/// Changes to one entry.
pub struct EntryMut<'a> {
    el: &'a mut Element,
}

impl<'a> EntryMut<'a> {
    /// Edits an entry element that is not in a database, such as a copy
    /// made to see what an edit would change.
    pub fn new(el: &'a mut Element) -> EntryMut<'a> {
        EntryMut { el }
    }
}

impl EntryMut<'_> {
    /// Sets a field, adding it when missing. `protected` decides whether the
    /// value is encrypted in the file.
    pub fn set_field(&mut self, key: &str, value: &str, protected: bool) {
        let existing = self
            .el
            .elements_mut()
            .find(|e| e.name == "String" && e.child_text("Key").as_deref() == Some(key));
        let target = match existing {
            Some(s) => s,
            None => {
                let mut s = Element::new("String");
                s.children
                    .push(Node::Element(Element::with_text("Key", key)));
                s.children.push(Node::Element(Element::new("Value")));
                self.el
                    .insert_element(s, &["String"], &["Binary", "AutoType", "History"]);
                self.el
                    .elements_mut()
                    .filter(|e| e.name == "String")
                    .find(|e| e.child_text("Key").as_deref() == Some(key))
                    .expect("just inserted")
            }
        };
        let v = match target.child_mut("Value") {
            Some(v) => v,
            None => {
                target.insert_element(Element::new("Value"), &["Key"], &[]);
                target.child_mut("Value").expect("just inserted")
            }
        };
        if protected {
            v.set_secret(value.as_bytes());
        } else {
            v.set_text(value);
        }
    }

    /// `false` when there was no such field.
    pub fn remove_field(&mut self, key: &str) -> bool {
        let pos = self.el.children.iter().position(
            |c| matches!(c, Node::Element(e) if e.name == "String" && e.child_text("Key").as_deref() == Some(key)),
        );
        pos.and_then(|i| self.el.remove_at(i)).is_some()
    }

    /// Renames a field in place. `false` when there is no field `from`, or
    /// already another one called `to`.
    pub fn rename_field(&mut self, from: &str, to: &str) -> bool {
        let has = |el: &Element, k: &str| {
            el.children_named("String")
                .any(|e| e.child_text("Key").as_deref() == Some(k))
        };
        if from == to {
            return has(self.el, from);
        }
        if has(self.el, to) {
            return false;
        }
        let Some(k) = self
            .el
            .elements_mut()
            .filter(|e| e.name == "String")
            .find(|e| e.child_text("Key").as_deref() == Some(from))
            .and_then(|e| e.child_mut("Key"))
        else {
            return false;
        };
        k.set_text(to);
        true
    }

    /// Replaces the tags, separated by `;` as KeePassXC and KeePass 2.47+
    /// write them.
    pub fn set_tags(&mut self, tags: &[String]) {
        let text = tags
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(";");
        if self.el.child("Tags").is_none() && text.is_empty() {
            return;
        }
        set_child_text(
            self.el,
            "Tags",
            &text,
            &[
                "UUID",
                "IconID",
                "CustomIconUUID",
                "ForegroundColor",
                "BackgroundColor",
                "OverrideURL",
                "QualityCheck",
            ],
        );
    }

    /// Sets when the entry expires; `None` means never. Switching expiry off
    /// keeps the old expiry time, as KeePass does.
    pub fn set_expiry(&mut self, when: Option<Timestamp>) {
        self.ensure_times();
        let t = self.el.child_mut("Times").expect("just ensured");
        if let Some(when) = when {
            set_child_text(
                t,
                "ExpiryTime",
                &when.to_base64(),
                &["LastAccessTime", "CreationTime", "LastModificationTime"],
            );
        }
        set_child_text(
            t,
            "Expires",
            if when.is_some() { "True" } else { "False" },
            &["ExpiryTime"],
        );
    }

    fn ensure_times(&mut self) {
        if self.el.child("Times").is_none() {
            self.el.insert_element(
                Element::new("Times"),
                &["Tags", "PreviousParentGroup"],
                &["String"],
            );
        }
    }

    /// Copies the entry as it is now into its history.
    pub fn push_history(&mut self) {
        let mut snapshot = self.el.clone();
        snapshot.remove_child("History");
        if self.el.child("History").is_none() {
            self.el.insert_element(Element::new("History"), &[], &[]);
        }
        let h = self.el.child_mut("History").expect("just ensured");
        h.insert_element(snapshot, &["Entry"], &[]);
    }

    /// Stamps the modification and access times with now.
    pub fn touch(&mut self) {
        let now = now_text();
        self.ensure_times();
        let t = self.el.child_mut("Times").expect("just ensured");
        set_child_text(t, "LastModificationTime", &now, &[]);
        set_child_text(t, "LastAccessTime", &now, &["LastModificationTime"]);
    }

    pub fn element(&mut self) -> &mut Element {
        self.el
    }
}

pub(crate) fn times_element(now: &str) -> Element {
    let mut t = Element::new("Times");
    for (n, v) in [
        ("LastModificationTime", now),
        ("CreationTime", now),
        ("LastAccessTime", now),
        ("ExpiryTime", now),
        ("Expires", "False"),
        ("UsageCount", "0"),
        ("LocationChanged", now),
    ] {
        t.children.push(Node::Element(Element::with_text(n, v)));
    }
    t
}

impl Database {
    pub fn entry_mut(&mut self, id: Uuid) -> Option<EntryMut<'_>> {
        let path = self.index.entries.get(&id)?;
        Some(EntryMut {
            el: at_mut(&mut self.root, path)?,
        })
    }

    /// A raw handle on `Meta`, for settings not covered here.
    pub fn meta_element_mut(&mut self) -> Option<&mut Element> {
        self.root.child_mut("Meta")
    }

    /// A raw handle on a group's element, for edits not covered here.
    pub fn group_element_mut(&mut self, id: Uuid) -> Option<&mut Element> {
        let path = self.index.groups.get(&id)?;
        at_mut(&mut self.root, path)
    }

    /// Drops the oldest history versions of an entry until it is within the
    /// database's `HistoryMaxItems` and `HistoryMaxSize`, as KeePass does
    /// after each edit. Returns how many were dropped.
    pub fn maintain_history(&mut self, id: Uuid) -> usize {
        let meta = self.meta();
        let (max_items, max_size) = (meta.history_max_items(), meta.history_max_size());
        let mut dropped = 0;
        loop {
            let Some(e) = self.entry(id) else {
                return dropped;
            };
            let versions: Vec<Entry<'_>> = e.history().collect();
            let too_many = max_items.is_some_and(|m| versions.len() > m);
            let too_big = !versions.is_empty()
                && max_size
                    .is_some_and(|m| versions.iter().map(Entry::approx_size).sum::<u64>() > m);
            if !too_many && !too_big {
                return dropped;
            }
            // the oldest by modification time; the first one on a tie
            let oldest = versions
                .iter()
                .enumerate()
                .min_by_key(|(i, v)| (v.times().last_modification, *i))
                .map_or(0, |(i, _)| i);
            let Some(path) = self.index.entries.get(&id).cloned() else {
                return dropped;
            };
            let Some(h) = at_mut(&mut self.root, &path).and_then(|el| el.child_mut("History"))
            else {
                return dropped;
            };
            let Some(pos) = h
                .children
                .iter()
                .enumerate()
                .filter(|(_, c)| matches!(c, Node::Element(e) if e.name == "Entry"))
                .nth(oldest)
                .map(|(i, _)| i)
            else {
                return dropped;
            };
            h.remove_at(pos);
            dropped += 1;
        }
    }

    /// Makes history version `index` (oldest first, as [`Entry::history`]
    /// lists them) the current one. The current version goes into the
    /// history first, then the history is trimmed to the database's limits.
    pub fn restore_history(&mut self, id: Uuid, index: usize) -> bool {
        let Some(old) = self
            .entry(id)
            .and_then(|e| e.history().nth(index))
            .map(|v| v.element().clone())
        else {
            return false;
        };
        let Some(path) = self.index.entries.get(&id).cloned() else {
            return false;
        };
        let Some(el) = at_mut(&mut self.root, &path) else {
            return false;
        };
        let mut cur = EntryMut { el };
        cur.push_history();
        let mut restored = old;
        restored.remove_child("History");
        set_child_text(&mut restored, "UUID", &id.to_base64(), &[]);
        if let Some(h) = cur.el.child("History") {
            restored.insert_element(h.clone(), &[], &[]);
        }
        crate::xml::reindent_like(&mut restored, cur.el);
        restored.attrs = cur.el.attrs.clone();
        *cur.el = restored;
        cur.touch();
        self.maintain_history(id);
        true
    }

    /// Adds an empty group at the end of `parent`'s subgroups.
    pub fn add_group(&mut self, parent: Uuid, name: &str) -> Option<Uuid> {
        let id = Uuid::random();
        let now = now_text();
        let mut g = Element::new("Group");
        for (n, v) in [
            ("UUID", id.to_base64()),
            ("Name", name.to_string()),
            ("Notes", String::new()),
            ("IconID", "48".to_string()),
        ] {
            g.children.push(Node::Element(Element::with_text(n, &v)));
        }
        g.children.push(Node::Element(times_element(&now)));
        for (n, v) in [
            ("IsExpanded", "True"),
            ("DefaultAutoTypeSequence", ""),
            ("EnableAutoType", "null"),
            ("EnableSearching", "null"),
            ("LastTopVisibleEntry", "AAAAAAAAAAAAAAAAAAAAAA=="),
        ] {
            g.children.push(Node::Element(Element::with_text(n, v)));
        }
        self.group_element_mut(parent)?
            .insert_element(g, &["Group"], &[]);
        self.reindex();
        Some(id)
    }

    /// Adds an entry with empty standard fields after `group`'s other entries.
    pub fn add_entry(&mut self, group: Uuid) -> Option<Uuid> {
        let id = Uuid::random();
        let now = now_text();
        let mut e = Element::new("Entry");
        e.children
            .push(Node::Element(Element::with_text("UUID", &id.to_base64())));
        e.children
            .push(Node::Element(Element::with_text("IconID", "0")));
        e.children.push(Node::Element(times_element(&now)));
        for k in ["Notes", "Password", "Title", "URL", "UserName"] {
            let mut s = Element::new("String");
            s.children.push(Node::Element(Element::with_text("Key", k)));
            let mut v = Element::new("Value");
            if k == "Password" {
                v.set_secret(b"");
            }
            s.children.push(Node::Element(v));
            e.children.push(Node::Element(s));
        }
        let mut at = Element::new("AutoType");
        at.children
            .push(Node::Element(Element::with_text("Enabled", "True")));
        at.children.push(Node::Element(Element::with_text(
            "DataTransferObfuscation",
            "0",
        )));
        e.children.push(Node::Element(at));
        self.group_element_mut(group)?
            .insert_element(e, &["Entry"], &["Group"]);
        self.reindex();
        Some(id)
    }

    /// Adds an attachment to the pool and links it to the entry.
    pub fn add_attachment(
        &mut self,
        entry: Uuid,
        name: &str,
        data: &[u8],
        protected: bool,
    ) -> bool {
        let index = self.binaries.len();
        let path = match self.index.entries.get(&entry) {
            Some(p) => p.clone(),
            None => return false,
        };
        let Some(el) = at_mut(&mut self.root, &path) else {
            return false;
        };
        let mut b = Element::new("Binary");
        b.children
            .push(Node::Element(Element::with_text("Key", name)));
        let mut v = Element::new("Value");
        v.set_attr("Ref", &index.to_string());
        b.children.push(Node::Element(v));
        el.insert_element(b, &["Binary", "String"], &["AutoType", "History"]);
        self.binaries.push(Binary {
            flags: u8::from(protected),
            data: zeroize::Zeroizing::new(data.to_vec()),
        });
        true
    }

    /// Moves an entry into another group, recording where it came from
    /// (`PreviousParentGroup`, KDBX 4.1) and when (`LocationChanged`).
    pub fn move_entry(&mut self, entry: Uuid, to: Uuid) -> bool {
        let (Some(from_path), Some(to_path)) = (
            self.index.entries.get(&entry).cloned(),
            self.index.groups.get(&to).cloned(),
        ) else {
            return false;
        };
        let (parent_path, i) = from_path.split_at(from_path.len() - 1);
        let Some(parent) = at_mut(&mut self.root, parent_path) else {
            return false;
        };
        let old_parent = uuid_of(parent);
        if old_parent == Some(to) {
            return true;
        }
        let Some(mut el) = parent.remove_at(i[0]) else {
            return false;
        };
        // children were removed: find the target again
        self.reindex();
        let _ = to_path;
        if let Some(p) = old_parent {
            set_child_text(
                &mut el,
                "PreviousParentGroup",
                &p.to_base64(),
                &["Tags", "QualityCheck", "OverrideURL"],
            );
        }
        if let Some(t) = el.child_mut("Times") {
            set_child_text(t, "LocationChanged", &now_text(), &["UsageCount"]);
        }
        let Some(target) = self.group_element_mut(to) else {
            return false;
        };
        target.insert_element(el, &["Entry"], &["Group"]);
        self.reindex();
        true
    }
}
