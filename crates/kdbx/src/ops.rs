//! Structural edits for M8: the recycle bin, groups, permanent deletion,
//! icons and attachments. Like the edits in `view.rs`, each touches only the
//! nodes it is about; untouched parts of the document stay as they were read.

use crate::db::Database;
use crate::types::{Timestamp, Uuid};
use crate::view::{EntryMut, at, at_mut, now_text, set_child_text, uuid_of, uuid_of_text};
use crate::xml::{Element, Node};

/// KeePass standard icon numbers used for new groups.
const ICON_RECYCLE_BIN: u32 = 43;

/// Child indices from `root` to the element at `path`'s parent, and the
/// index of the element itself.
fn split(path: &[usize]) -> Option<(&[usize], usize)> {
    let (last, parent) = path.split_last()?;
    Some((parent, *last))
}

impl Database {
    fn root_group_id(&self) -> Option<Uuid> {
        self.root_group().and_then(|g| g.uuid())
    }

    /// Whether `group` is `ancestor` or lies anywhere below it.
    fn is_within(&self, group: Uuid, ancestor: Uuid) -> bool {
        let (Some(g), Some(a)) = (
            self.index.groups.get(&group),
            self.index.groups.get(&ancestor),
        ) else {
            return false;
        };
        g.starts_with(a)
    }

    /// The group a group is in; `None` for the root group.
    pub fn parent_of_group(&self, id: Uuid) -> Option<Uuid> {
        let path = self.index.groups.get(&id)?;
        let (parent, _) = split(path)?;
        let parent_el = at(&self.root, parent)?;
        (parent_el.name == "Group")
            .then(|| uuid_of(parent_el))
            .flatten()
    }

    /// The recycle bin, made on first use when the database has none (or
    /// its `RecycleBinUUID` points at nothing). `None` when the database has
    /// the recycle bin turned off; then deleting is permanent.
    pub fn ensure_recycle_bin(&mut self, name: &str) -> Option<Uuid> {
        let meta = self.meta();
        if !meta.recycle_bin_enabled() {
            return None;
        }
        if let Some(id) = meta.recycle_bin()
            && self.index.groups.contains_key(&id)
        {
            return Some(id);
        }
        let root = self.root_group_id()?;
        let id = self.add_group(root, name)?;
        if let Some(g) = self.group_element_mut(id) {
            set_child_text(
                g,
                "IconID",
                &ICON_RECYCLE_BIN.to_string(),
                &["Notes", "Name"],
            );
            set_child_text(g, "EnableAutoType", "false", &["DefaultAutoTypeSequence"]);
            set_child_text(g, "EnableSearching", "false", &["EnableAutoType"]);
        }
        let now = now_text();
        let meta = self.meta_element_mut()?;
        set_child_text(
            meta,
            "RecycleBinUUID",
            &id.to_base64(),
            &["RecycleBinEnabled"],
        );
        set_child_text(meta, "RecycleBinChanged", &now, &["RecycleBinUUID"]);
        Some(id)
    }

    /// Renames a group and stamps its modification time.
    pub fn rename_group(&mut self, id: Uuid, name: &str) -> bool {
        let Some(g) = self.group_element_mut(id) else {
            return false;
        };
        set_child_text(g, "Name", name, &["UUID"]);
        if let Some(t) = g.child_mut("Times") {
            let now = now_text();
            set_child_text(t, "LastModificationTime", &now, &[]);
        }
        true
    }

    /// Moves a group (with everything in it) into another group. Refused for
    /// the root group, and for a target inside the group itself.
    pub fn move_group(&mut self, id: Uuid, to: Uuid) -> bool {
        if Some(id) == self.root_group_id() || self.is_within(to, id) {
            return false;
        }
        if !self.index.groups.contains_key(&to) {
            return false;
        }
        let Some(path) = self.index.groups.get(&id).cloned() else {
            return false;
        };
        let Some((parent_path, i)) = split(&path) else {
            return false;
        };
        let Some(parent) = at_mut(&mut self.root, parent_path) else {
            return false;
        };
        let old_parent = uuid_of(parent);
        if old_parent == Some(to) {
            return true;
        }
        let Some(mut el) = parent.remove_at(i) else {
            return false;
        };
        self.reindex();
        if let Some(p) = old_parent {
            set_child_text(
                &mut el,
                "PreviousParentGroup",
                &p.to_base64(),
                &["Tags", "CustomIconUUID", "IconID", "Notes", "Name"],
            );
        }
        if let Some(t) = el.child_mut("Times") {
            set_child_text(t, "LocationChanged", &now_text(), &["UsageCount"]);
        }
        let Some(target) = self.group_element_mut(to) else {
            return false;
        };
        target.insert_element(el, &["Group"], &[]);
        self.reindex();
        true
    }

    /// Where an entry or group was before it was last moved
    /// (`PreviousParentGroup`, KDBX 4.1), if that group still exists.
    pub fn previous_parent(&self, id: Uuid) -> Option<Uuid> {
        let path = self
            .index
            .entries
            .get(&id)
            .or_else(|| self.index.groups.get(&id))?;
        let el = at(&self.root, path)?;
        uuid_of_text(el, "PreviousParentGroup").filter(|g| self.index.groups.contains_key(g))
    }

    /// Removes an entry for good and records it in `DeletedObjects`.
    pub fn delete_entry(&mut self, id: Uuid) -> bool {
        let Some(path) = self.index.entries.get(&id).cloned() else {
            return false;
        };
        let Some((parent_path, i)) = split(&path) else {
            return false;
        };
        let removed = at_mut(&mut self.root, parent_path).and_then(|p| p.remove_at(i));
        let Some(el) = removed else { return false };
        let mut refs = binary_refs(&el);
        self.reindex();
        self.record_deleted(&[id]);
        refs.sort_unstable();
        refs.dedup();
        // highest first: dropping one renumbers those after it
        for r in refs.into_iter().rev() {
            self.drop_binary_if_unused(r);
        }
        true
    }

    /// Removes a group, its subgroups and all their entries for good, and
    /// records every one of them in `DeletedObjects`. Not the root group.
    pub fn delete_group(&mut self, id: Uuid) -> bool {
        if Some(id) == self.root_group_id() {
            return false;
        }
        let Some(path) = self.index.groups.get(&id).cloned() else {
            return false;
        };
        let Some((parent_path, i)) = split(&path) else {
            return false;
        };
        let removed = at_mut(&mut self.root, parent_path).and_then(|p| p.remove_at(i));
        let Some(el) = removed else { return false };
        let mut gone = Vec::new();
        collect_ids(&el, &mut gone);
        let refs = binary_refs(&el);
        self.reindex();
        self.record_deleted(&gone);
        // a deleted recycle bin is no longer the recycle bin
        if self.meta().recycle_bin() == Some(id)
            && let Some(meta) = self.meta_element_mut()
        {
            set_child_text(meta, "RecycleBinUUID", &Uuid([0; 16]).to_base64(), &[]);
            set_child_text(meta, "RecycleBinChanged", &now_text(), &["RecycleBinUUID"]);
        }
        let mut refs = refs;
        refs.sort_unstable();
        refs.dedup();
        // highest first: dropping one renumbers those after it
        for r in refs.into_iter().rev() {
            self.drop_binary_if_unused(r);
        }
        true
    }

    /// Adds `<DeletedObject>` records to `Root/DeletedObjects`, so that
    /// syncing programs learn these items were deleted, not just missing.
    fn record_deleted(&mut self, ids: &[Uuid]) {
        let Some(root) = self.root.child_mut("Root") else {
            return;
        };
        if root.child("DeletedObjects").is_none() {
            root.insert_element(Element::new("DeletedObjects"), &["Group"], &[]);
        }
        let list = root.child_mut("DeletedObjects").expect("just ensured");
        let now = now_text();
        for id in ids {
            let mut d = Element::new("DeletedObject");
            d.children
                .push(Node::Element(Element::with_text("UUID", &id.to_base64())));
            d.children
                .push(Node::Element(Element::with_text("DeletionTime", &now)));
            list.insert_element(d, &["DeletedObject"], &[]);
        }
    }

    /// Removes pool item `index` when no entry (history included) refers to
    /// it any more, and renumbers the references after it.
    fn drop_binary_if_unused(&mut self, index: usize) {
        if index >= self.binaries.len() {
            return;
        }
        let mut refs = Vec::new();
        refs_in(&self.root, &mut refs);
        if refs.contains(&index) {
            return;
        }
        self.binaries.remove(index);
        visit_refs_mut(&mut self.root, &mut |r| {
            if *r > index {
                *r -= 1;
            }
        });
    }

    /// Removes an attachment from an entry (the current version only; older
    /// versions keep theirs). `false` when there is no such attachment.
    pub fn remove_attachment(&mut self, entry: Uuid, name: &str) -> bool {
        let Some(path) = self.index.entries.get(&entry).cloned() else {
            return false;
        };
        let Some(el) = at_mut(&mut self.root, &path) else {
            return false;
        };
        let pos = el.children.iter().position(|c| {
            matches!(c, Node::Element(b) if b.name == "Binary"
                && b.child_text("Key").as_deref() == Some(name))
        });
        let Some(removed) = pos.and_then(|i| el.remove_at(i)) else {
            return false;
        };
        let mut refs = binary_refs(&removed);
        refs.sort_unstable();
        refs.dedup();
        for r in refs.into_iter().rev() {
            self.drop_binary_if_unused(r);
        }
        true
    }

    /// Records a new master key in `Meta`: `MasterKeyChanged` becomes `at`,
    /// and a pending "change the master key next time" request
    /// (`MasterKeyChangeForceOnce`, which KeePass only writes when set) is done.
    pub fn note_master_key_changed(&mut self, at: Timestamp) {
        let Some(meta) = self.meta_element_mut() else {
            return;
        };
        set_child_text(meta, "MasterKeyChanged", &at.to_base64(), &["Color"]);
        meta.remove_child("MasterKeyChangeForceOnce");
    }

    /// Records changed encryption settings in `Meta` (`SettingsChanged`, which
    /// KeePass uses when merging).
    pub fn note_settings_changed(&mut self, at: Timestamp) {
        let Some(meta) = self.meta_element_mut() else {
            return;
        };
        set_child_text(
            meta,
            "SettingsChanged",
            &at.to_base64(),
            &["HistoryMaxSize"],
        );
    }
}

impl EntryMut<'_> {
    /// Sets a standard icon and drops a custom one, as KeePass does when a
    /// standard icon is picked.
    pub fn set_icon(&mut self, icon: u32) {
        let el = self.element();
        set_child_text(el, "IconID", &icon.to_string(), &["UUID"]);
        el.remove_child("CustomIconUUID");
    }
}

/// UUIDs of a group, its subgroups and their entries (not history versions).
fn collect_ids(el: &Element, out: &mut Vec<Uuid>) {
    if let Some(id) = uuid_of(el) {
        out.push(id);
    }
    for c in el.elements() {
        if c.name == "Group" || c.name == "Entry" {
            collect_ids(c, out);
        }
    }
}

/// Pool references (`<Binary><Value Ref="n"/>`) anywhere inside `el`.
fn binary_refs(el: &Element) -> Vec<usize> {
    let mut out = Vec::new();
    refs_in(el, &mut out);
    out
}

fn refs_in(el: &Element, out: &mut Vec<usize>) {
    if el.name == "Binary"
        && let Some(r) = el
            .child("Value")
            .and_then(|v| v.attr("Ref"))
            .and_then(|r| r.trim().parse().ok())
    {
        out.push(r);
    }
    for c in el.elements() {
        refs_in(c, out);
    }
}

fn visit_refs_mut(el: &mut Element, f: &mut dyn FnMut(&mut usize)) {
    if el.name == "Binary"
        && let Some(v) = el.child_mut("Value")
        && let Some(mut r) = v.attr("Ref").and_then(|r| r.trim().parse::<usize>().ok())
    {
        let before = r;
        f(&mut r);
        if r != before {
            v.set_attr("Ref", &r.to_string());
        }
    }
    for c in el.elements_mut() {
        visit_refs_mut(c, f);
    }
}
