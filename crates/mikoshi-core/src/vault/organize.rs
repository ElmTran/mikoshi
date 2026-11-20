//! Deleting, the recycle bin, groups, moving entries, and attachments
//! (features.md §3.8, M8).
//!
//! Deleting something outside the recycle bin moves it there (the bin is made
//! on first use). Deleting inside the bin, or in a database with the bin
//! turned off, is permanent; the UI asks first, using
//! [`Vault::deletes_permanently`]. Permanent deletes are recorded in
//! `DeletedObjects`, as KeePass does.

use std::collections::HashSet;

use kdbx::Uuid;
use serde::Serialize;
use zeroize::Zeroizing;

use super::Vault;
use super::edit::EditError;
use crate::model::GroupKind;

/// What a delete did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Deleted {
    /// moved into the recycle bin
    ToRecycleBin,
    /// gone for good
    Permanently,
}

impl Vault {
    fn entry_uuid(&self, id: &str) -> Result<Uuid, EditError> {
        self.entries
            .get(id)
            .map(|(u, _)| *u)
            .ok_or(EditError::NotFound)
    }

    fn group_at(&self, id: &str) -> Result<usize, EditError> {
        self.group_index.get(id).copied().ok_or(EditError::NotFound)
    }

    fn entry_in_bin(&self, id: &str) -> bool {
        self.entries
            .get(id)
            .is_some_and(|(_, g)| self.groups[*g].in_bin)
    }

    /// Whether deleting these entries and groups removes them for good (they
    /// are in the recycle bin, or the database has it turned off), so the UI
    /// asks before going ahead.
    pub fn deletes_permanently(&self, entries: &[String], groups: &[String]) -> bool {
        !self.db.meta().recycle_bin_enabled()
            || entries.iter().any(|e| self.entry_in_bin(e))
            || groups.iter().any(|g| {
                self.group_index
                    .get(g)
                    .is_some_and(|&i| self.groups[i].in_bin)
            })
    }

    /// Deletes entries: into the recycle bin, or for good when they are
    /// already in it (or the bin is turned off). `bin_name` names the bin
    /// when it has to be made.
    pub fn delete_entries(&mut self, ids: &[String], bin_name: &str) -> Result<Deleted, EditError> {
        let uuids = ids
            .iter()
            .map(|id| self.entry_uuid(id))
            .collect::<Result<Vec<_>, _>>()?;
        let permanent = self.deletes_permanently(ids, &[]);
        if permanent {
            for u in uuids {
                self.db.delete_entry(u);
            }
        } else {
            let bin = self
                .db
                .ensure_recycle_bin(bin_name)
                .ok_or(EditError::NotFound)?;
            for u in uuids {
                self.db.move_entry(u, bin);
            }
        }
        self.edits += 1;
        self.relayout();
        Ok(if permanent {
            Deleted::Permanently
        } else {
            Deleted::ToRecycleBin
        })
    }

    /// Deletes a group with everything in it, the same way as entries. The
    /// root group cannot be deleted; deleting the recycle bin empties it and
    /// removes it.
    pub fn delete_group(&mut self, id: &str, bin_name: &str) -> Result<Deleted, EditError> {
        let i = self.group_at(id)?;
        let node = &self.groups[i];
        if node.kind == GroupKind::Root {
            return Err(EditError::NotAllowed);
        }
        let uuid = node.id;
        let permanent = self.deletes_permanently(&[], &[id.to_string()]);
        if permanent {
            self.db.delete_group(uuid);
        } else {
            let bin = self
                .db
                .ensure_recycle_bin(bin_name)
                .ok_or(EditError::NotFound)?;
            self.db.move_group(uuid, bin);
        }
        self.edits += 1;
        self.relayout();
        Ok(if permanent {
            Deleted::Permanently
        } else {
            Deleted::ToRecycleBin
        })
    }

    /// Takes an entry or group out of the recycle bin: back to the group it
    /// was deleted from, or to the root group when that one is gone or is in
    /// the bin itself.
    pub fn restore(&mut self, id: &str) -> Result<(), EditError> {
        let (uuid, is_entry) = if let Some((u, _)) = self.entries.get(id) {
            (*u, true)
        } else {
            (self.groups[self.group_at(id)?].id, false)
        };
        let in_bin = if is_entry {
            self.entry_in_bin(id)
        } else {
            self.groups[self.group_at(id)?].in_bin
        };
        let bin = self.db.meta().recycle_bin();
        if !in_bin || Some(uuid) == bin {
            return Err(EditError::NotAllowed);
        }
        let root = self
            .groups
            .first()
            .map(|g| g.id)
            .ok_or(EditError::NotFound)?;
        let target = self
            .db
            .previous_parent(uuid)
            .filter(|p| {
                self.group_index
                    .get(&p.to_string())
                    .is_some_and(|&i| !self.groups[i].in_bin)
            })
            .unwrap_or(root);
        let moved = if is_entry {
            self.db.move_entry(uuid, target)
        } else {
            self.db.move_group(uuid, target)
        };
        if !moved {
            return Err(EditError::NotAllowed);
        }
        self.edits += 1;
        self.relayout();
        Ok(())
    }

    /// Deletes everything in the recycle bin for good. Returns how many
    /// entries and groups were removed directly under it.
    pub fn empty_recycle_bin(&mut self) -> Result<usize, EditError> {
        let Some(bin) = self.db.meta().recycle_bin() else {
            return Ok(0);
        };
        let Some(group) = self.db.group(bin) else {
            return Ok(0);
        };
        let entries: Vec<Uuid> = group.entries().filter_map(|e| e.uuid()).collect();
        let groups: Vec<Uuid> = group.groups().filter_map(|g| g.uuid()).collect();
        let n = entries.len() + groups.len();
        for e in entries {
            self.db.delete_entry(e);
        }
        for g in groups {
            self.db.delete_group(g);
        }
        if n > 0 {
            self.edits += 1;
            self.relayout();
        }
        Ok(n)
    }

    // ------------------------------------------------------------ groups

    /// Adds a group under `parent` and returns its id. Not inside the bin.
    pub fn create_group(&mut self, parent: &str, name: &str) -> Result<String, EditError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EditError::EmptyName);
        }
        let i = self.group_at(parent)?;
        if self.groups[i].in_bin {
            return Err(EditError::RecycleBin);
        }
        let id = self
            .db
            .add_group(self.groups[i].id, name)
            .ok_or(EditError::NotFound)?;
        self.edits += 1;
        self.relayout();
        Ok(id.to_string())
    }

    pub fn rename_group(&mut self, id: &str, name: &str) -> Result<(), EditError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EditError::EmptyName);
        }
        let i = self.group_at(id)?;
        let uuid = self.groups[i].id;
        let unchanged = self.db.group(uuid).is_some_and(|g| g.name() == name);
        if unchanged {
            return Ok(());
        }
        self.db.rename_group(uuid, name);
        self.edits += 1;
        Ok(())
    }

    /// Moves a group into another one. Not the root or the recycle bin, not
    /// into itself or below itself, and not into the bin (that is deleting).
    pub fn move_group(&mut self, id: &str, to: &str) -> Result<(), EditError> {
        let (i, t) = (self.group_at(id)?, self.group_at(to)?);
        let node = &self.groups[i];
        if node.kind != GroupKind::Normal || self.groups[t].in_bin {
            return Err(EditError::NotAllowed);
        }
        if !self.db.move_group(node.id, self.groups[t].id) {
            return Err(EditError::NotAllowed);
        }
        self.edits += 1;
        self.relayout();
        Ok(())
    }

    /// Moves entries into a group (not into the bin: that is deleting).
    pub fn move_entries(&mut self, ids: &[String], to: &str) -> Result<(), EditError> {
        let t = self.group_at(to)?;
        if self.groups[t].in_bin {
            return Err(EditError::NotAllowed);
        }
        let uuids = ids
            .iter()
            .map(|id| self.entry_uuid(id))
            .collect::<Result<Vec<_>, _>>()?;
        let target = self.groups[t].id;
        let mut moved = false;
        for u in uuids {
            let here = self.db.parent_of_entry(u).and_then(|g| g.uuid());
            if here != Some(target) {
                self.db.move_entry(u, target);
                moved = true;
            }
        }
        if moved {
            self.edits += 1;
            self.relayout();
        }
        Ok(())
    }

    // ------------------------------------------------------------ attachments

    /// An attachment's bytes, for exporting it to a file.
    pub fn attachment(&self, id: &str, name: &str) -> Option<Zeroizing<Vec<u8>>> {
        let (e, _) = self.entry_ref(id)?;
        let a = e.attachments().find(|a| a.name == name)?;
        Some(Zeroizing::new(a.data?.into_owned()))
    }

    /// Attaches a file to an entry; the old version goes into the history.
    /// A name the entry already uses gets " (2)", " (3)", ... added before
    /// the extension. Returns the name used.
    pub fn add_attachment(
        &mut self,
        id: &str,
        name: &str,
        data: &[u8],
    ) -> Result<String, EditError> {
        let uuid = self.entry_uuid(id)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(EditError::EmptyName);
        }
        let taken: HashSet<String> = self
            .db
            .entry(uuid)
            .ok_or(EditError::NotFound)?
            .attachments()
            .map(|a| a.name.into_owned())
            .collect();
        let name = free_name(name, &taken);
        let mut e = self.db.entry_mut(uuid).ok_or(EditError::NotFound)?;
        e.push_history();
        e.touch();
        // KeePass marks attachments protected in memory only when asked; the
        // file content is encrypted with the database either way
        self.db.add_attachment(uuid, &name, data, false);
        self.db.maintain_history(uuid);
        self.edits += 1;
        Ok(name)
    }

    /// Removes an attachment from an entry; the old version, with the
    /// attachment, goes into the history.
    pub fn remove_attachment(&mut self, id: &str, name: &str) -> Result<(), EditError> {
        let uuid = self.entry_uuid(id)?;
        let has = self
            .db
            .entry(uuid)
            .is_some_and(|e| e.attachments().any(|a| a.name == name));
        if !has {
            return Err(EditError::NotFound);
        }
        let mut e = self.db.entry_mut(uuid).ok_or(EditError::NotFound)?;
        e.push_history();
        e.touch();
        self.db.remove_attachment(uuid, name);
        self.db.maintain_history(uuid);
        self.edits += 1;
        Ok(())
    }
}

/// `name`, or `stem (n).ext` when it is taken.
fn free_name(name: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|c| !taken.contains(c))
        .expect("an unused name exists")
}

#[cfg(test)]
mod tests {
    use super::free_name;
    use std::collections::HashSet;

    #[test]
    fn taken_names_get_a_number() {
        let taken: HashSet<String> = ["a.txt", "a (2).txt", "noext"].map(String::from).into();
        assert_eq!(free_name("b.txt", &taken), "b.txt");
        assert_eq!(free_name("a.txt", &taken), "a (3).txt");
        assert_eq!(free_name("noext", &taken), "noext (2)");
        assert_eq!(
            free_name(".hidden", &HashSet::from([".hidden".to_string()])),
            ".hidden (2)"
        );
    }
}
