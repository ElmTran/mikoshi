//! What the kdbx crate reads from a file, in the shapes the checks compare:
//! attachment contents keyed like [`crate::canon`] paths, the entry paths
//! keepassxc-cli uses, and differences between two XML trees.

use std::collections::BTreeMap;

use kdbx::xml::{Element, Node};
use kdbx::{Database, Entry, Group};

use crate::canon::short_hash;

/// Attachment contents per entry, keyed like the canonical paths:
/// `.../Entry[uuid]/Binary[name]` and `.../Entry[uuid]/History/Entry#i/Binary[name]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentInfo {
    pub len: usize,
    pub hash: String,
}

fn seg(kind: &str, id: Option<kdbx::Uuid>) -> String {
    format!("{kind}[{}]", id.map(|u| u.to_base64()).unwrap_or_default())
}

pub fn attachments(db: &Database) -> BTreeMap<String, AttachmentInfo> {
    let mut out = BTreeMap::new();
    fn collect(out: &mut BTreeMap<String, AttachmentInfo>, path: &str, e: &Entry<'_>) {
        for a in e.attachments() {
            let data = a.data.unwrap_or_default();
            out.insert(
                format!("{path}/Binary[{}]", a.name),
                AttachmentInfo {
                    len: data.len(),
                    hash: short_hash(&data),
                },
            );
        }
    }
    fn walk(out: &mut BTreeMap<String, AttachmentInfo>, prefix: &str, g: &Group<'_>) {
        let path = format!("{prefix}/{}", seg("Group", g.uuid()));
        for child in g.groups() {
            walk(out, &path, &child);
        }
        for e in g.entries() {
            let ep = format!("{path}/{}", seg("Entry", e.uuid()));
            collect(out, &ep, &e);
            for (i, h) in e.history().enumerate() {
                collect(out, &format!("{ep}/History/Entry#{i}"), &h);
            }
        }
    }
    if let Some(root) = db.root_group() {
        walk(&mut out, "Root", &root);
    }
    out
}

/// keepassxc-cli addresses entries by "Group/Sub/Title" below the root group.
pub fn cli_entry_paths(db: &Database) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    fn walk(
        out: &mut BTreeMap<String, String>,
        canon_prefix: &str,
        cli_prefix: &str,
        g: &Group<'_>,
        is_root: bool,
    ) {
        let path = format!("{canon_prefix}/{}", seg("Group", g.uuid()));
        let cli = if is_root {
            String::new()
        } else if cli_prefix.is_empty() {
            g.name().into_owned()
        } else {
            format!("{cli_prefix}/{}", g.name())
        };
        for child in g.groups() {
            walk(out, &path, &cli, &child, false);
        }
        for e in g.entries() {
            let title = e.get("Title").unwrap_or_default();
            let cli_path = if cli.is_empty() {
                title.into_owned()
            } else {
                format!("{cli}/{title}")
            };
            out.insert(format!("{path}/{}", seg("Entry", e.uuid())), cli_path);
        }
    }
    if let Some(root) = db.root_group() {
        walk(&mut out, "Root", "", &root, true);
    }
    out
}

/// Where two trees differ, as element paths (`KeePassFile/Root/Group[2]/...`,
/// child positions in brackets). Never includes values. At most `limit`.
pub fn tree_diff(a: &Element, b: &Element, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    fn kind(n: &Node) -> &'static str {
        match n {
            Node::Element(_) => "element",
            Node::Text(_) => "text",
            Node::Secret(_) => "protected value",
            Node::Comment(_) => "comment",
            Node::Pi(..) => "processing instruction",
        }
    }
    fn walk(a: &Element, b: &Element, path: &str, out: &mut Vec<String>, limit: usize) {
        if out.len() >= limit {
            return;
        }
        if a.name != b.name {
            out.push(format!("{path}: element renamed"));
            return;
        }
        if a.attrs != b.attrs {
            out.push(format!("{path}: attributes differ"));
        }
        if a.children.len() != b.children.len() {
            out.push(format!(
                "{path}: {} children before, {} after",
                a.children.len(),
                b.children.len()
            ));
            return;
        }
        for (i, (x, y)) in a.children.iter().zip(&b.children).enumerate() {
            match (x, y) {
                (Node::Element(x), Node::Element(y)) => {
                    walk(x, y, &format!("{path}/{}[{i}]", x.name), out, limit)
                }
                _ if x == y => {}
                _ if kind(x) == kind(y) => out.push(format!("{path}: {} #{i} differs", kind(x))),
                _ => out.push(format!(
                    "{path}: child #{i} is a {} before, a {} after",
                    kind(x),
                    kind(y)
                )),
            }
        }
    }
    walk(a, b, &a.name, &mut out, limit);
    out
}

/// A copy of the tree without the groups and entries whose UUID is in
/// `ids`, together with the whitespace in front of each.
pub fn without(el: &Element, ids: &[kdbx::Uuid]) -> Element {
    let mut out = el.clone();
    fn strip(el: &mut Element, ids: &[kdbx::Uuid]) {
        let mut i = 0;
        while i < el.children.len() {
            let hit = matches!(&el.children[i], Node::Element(e)
                if (e.name == "Group" || e.name == "Entry")
                    && e.child_text("UUID")
                        .and_then(|u| kdbx::Uuid::from_base64(&u))
                        .is_some_and(|u| ids.contains(&u)));
            if hit {
                let ws_before =
                    i > 0 && matches!(&el.children[i - 1], Node::Text(t) if t.trim().is_empty());
                el.remove_at(i);
                if ws_before {
                    i -= 1;
                }
                continue;
            }
            if let Node::Element(e) = &mut el.children[i] {
                strip(e, ids);
            }
            i += 1;
        }
    }
    strip(&mut out, ids);
    out
}
