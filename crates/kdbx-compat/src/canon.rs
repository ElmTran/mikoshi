//! Canonical form of a KeePass XML document, for comparing two documents
//! while ignoring differences that carry no data (element order where KeePass
//! does not define one, empty vs. missing elements, timestamp encoding, tag
//! separators, ...).
//!
//! A document becomes a flat map from a stable path to a value. Groups and
//! entries are keyed by UUID, strings by key, custom data by key, history
//! entries and auto-type associations by position.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{Duration, NaiveDate, NaiveDateTime};

pub const EXISTS: &str = "(exists)";
pub const PROTECTED: &str = "(protected)";

/// Flattened document: path -> value.
#[derive(Debug, Default, Clone)]
pub struct Canon {
    pub records: BTreeMap<String, String>,
    /// UUID segment -> human-readable label, for display only.
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Decrypted inner XML as stored in the file. Protected values are still
    /// encrypted with the inner stream cipher, so only their presence is kept.
    Raw,
    /// keepassxc-cli export. Protected values are plaintext.
    Export,
}

pub fn parse(xml: &str, source: Source) -> Result<Canon, String> {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: u32::MAX,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(xml, opts).map_err(|e| e.to_string())?;
    let mut canon = Canon::default();
    walk(doc.root_element(), "", source, &mut canon);
    if source == Source::Export {
        // KeePassXC stamps this with the current time on every export
        canon
            .records
            .retain(|k, _| !k.ends_with("Item[_LAST_MODIFIED]/Value"));
    }
    Ok(canon)
}

fn child_text<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|c| c.has_tag_name(name))
        .map(|c| c.text().unwrap_or(""))
}

fn elements<'a>(node: roxmltree::Node<'a, 'a>) -> impl Iterator<Item = roxmltree::Node<'a, 'a>> {
    node.children().filter(|c| c.is_element())
}

/// Segment name for an element, plus the name of the child element that was
/// folded into it (and must not be emitted again).
fn segment(
    node: roxmltree::Node<'_, '_>,
    index_among_same: usize,
) -> (String, Option<&'static str>) {
    let name = node.tag_name().name();
    let parent = node
        .parent_element()
        .map(|p| p.tag_name().name())
        .unwrap_or("");
    let keyed = |child: &'static str| {
        let key = child_text(node, child).unwrap_or("").to_string();
        (format!("{name}[{key}]"), Some(child))
    };
    match (parent, name) {
        ("History", "Entry") => (format!("Entry#{index_among_same}"), None),
        (_, "Group") | (_, "Entry") | ("CustomIcons", "Icon") | (_, "DeletedObject") => {
            keyed("UUID")
        }
        ("Entry", "String") | ("Entry", "Binary") | ("CustomData", "Item") => keyed("Key"),
        ("AutoType", "Association") => (format!("Association#{index_among_same}"), None),
        _ if index_among_same > 0 => (format!("{name}#{index_among_same}"), None),
        _ => (name.to_string(), None),
    }
}

fn skip(path: &str, parent: &str, name: &str) -> bool {
    matches!(
        (parent, name),
        ("Meta", "Generator") | ("Meta", "HeaderHash") | ("Meta", "Binaries")
    ) || (path.ends_with("/Meta") && name == "Generator")
}

fn walk(node: roxmltree::Node<'_, '_>, prefix: &str, source: Source, canon: &mut Canon) {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let parent_name = node.tag_name().name();
    let mut child_groups = Vec::new();
    let mut child_entries = Vec::new();

    for child in elements(node) {
        let name = child.tag_name().name();
        if skip(prefix, parent_name, name) {
            continue;
        }
        let idx = seen.entry(name).or_insert(0);
        let (seg, folded) = segment(child, *idx);
        *idx += 1;
        let path = if prefix.is_empty() {
            seg.clone()
        } else {
            format!("{prefix}/{seg}")
        };

        match name {
            "Group" => child_groups.push(seg.clone()),
            "Entry" if parent_name == "Group" => child_entries.push(seg.clone()),
            _ => {}
        }
        if folded.is_some() || seg.contains('#') {
            canon.records.insert(path.clone(), EXISTS.to_string());
        }
        remember_label(child, &seg, canon);

        let has_children = elements(child).next().is_some();
        if has_children {
            walk_with_fold(child, &path, folded, source, canon);
        } else {
            leaf(child, &path, source, canon);
        }
    }

    if parent_name == "Group" {
        if !child_groups.is_empty() {
            canon
                .records
                .insert(format!("{prefix}/@groups"), child_groups.join(" "));
        }
        if !child_entries.is_empty() {
            canon
                .records
                .insert(format!("{prefix}/@entries"), child_entries.join(" "));
        }
    }
}

fn walk_with_fold(
    node: roxmltree::Node<'_, '_>,
    path: &str,
    folded: Option<&str>,
    source: Source,
    canon: &mut Canon,
) {
    // Same as walk(), but the element that was folded into the segment name
    // (UUID / Key) is not repeated as a record.
    let mut sub = Canon::default();
    walk(node, path, source, &mut sub);
    if let Some(folded) = folded {
        let folded_path = format!("{path}/{folded}");
        sub.records.remove(&folded_path);
    }
    canon.records.extend(sub.records);
    canon.labels.extend(sub.labels);
}

fn remember_label(node: roxmltree::Node<'_, '_>, seg: &str, canon: &mut Canon) {
    let label = match node.tag_name().name() {
        "Group" => child_text(node, "Name").map(|n| format!("group \"{n}\"")),
        "Entry" => node
            .children()
            .filter(|c| c.has_tag_name("String"))
            .find(|s| child_text(*s, "Key") == Some("Title"))
            .and_then(|s| child_text(s, "Value"))
            .map(|t| format!("entry \"{t}\"")),
        _ => None,
    };
    if let Some(label) = label {
        canon.labels.entry(seg.to_string()).or_insert(label);
    }
}

fn leaf(node: roxmltree::Node<'_, '_>, path: &str, source: Source, canon: &mut Canon) {
    let name = node.tag_name().name();
    let parent = node
        .parent_element()
        .map(|p| p.tag_name().name())
        .unwrap_or("");
    let text = node.text().unwrap_or("");

    // attributes
    let mut protected = false;
    for attr in node.attributes() {
        match (parent, name, attr.name()) {
            ("String", "Value", "Protected") | ("String", "Value", "ProtectInMemory") => {
                protected = attr.value().eq_ignore_ascii_case("true");
            }
            ("Binary", "Value", "Ref") => {
                // attachment pool indices may be renumbered on save; contents
                // are compared separately
                canon
                    .records
                    .insert(format!("{path}@Ref"), EXISTS.to_string());
            }
            _ => {
                canon
                    .records
                    .insert(format!("{path}@{}", attr.name()), attr.value().to_string());
            }
        }
    }
    if protected {
        canon
            .records
            .insert(format!("{path}@Protected"), "True".to_string());
    }

    let value = if protected && source == Source::Raw {
        if text.is_empty() {
            None
        } else {
            Some(PROTECTED.to_string())
        }
    } else {
        normalize(parent, name, text)
    };
    if let Some(v) = value {
        canon.records.insert(path.to_string(), v);
    }
}

const NIL_UUID: &str = "AAAAAAAAAAAAAAAAAAAAAA==";

fn is_time(name: &str) -> bool {
    name.ends_with("Time") || name.ends_with("Changed")
}

fn normalize(parent: &str, name: &str, text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    if name == "Value" && parent == "String" {
        // string field values are compared verbatim, whitespace included
        return Some(text.to_string());
    }
    if text == "null" {
        return None;
    }
    if text == NIL_UUID {
        return None;
    }
    if text.eq_ignore_ascii_case("true") {
        return Some("True".into());
    }
    if text.eq_ignore_ascii_case("false") {
        return Some("False".into());
    }
    if is_time(name) {
        return Some(normalize_time(text));
    }
    match name {
        "Tags" => {
            let tags: Vec<&str> = text
                .split([';', ','])
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .collect();
            (!tags.is_empty()).then(|| tags.join(";"))
        }
        "Data" if parent == "Icon" => Some(format!(
            "{} bytes, {}",
            B64.decode(text).map(|d| d.len()).unwrap_or(0),
            short_hash(text.as_bytes())
        )),
        "ForegroundColor" | "BackgroundColor" | "Color" => Some(text.to_ascii_uppercase()),
        _ => Some(text.to_string()),
    }
}

fn epoch() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(1, 1, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .unwrap_or_default()
}

pub fn normalize_time(text: &str) -> String {
    if let Ok(t) = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%SZ") {
        return t.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    }
    if let Ok(bytes) = B64.decode(text)
        && let Ok(arr) = <[u8; 8]>::try_from(bytes.as_slice())
    {
        let t = epoch() + Duration::seconds(i64::from_le_bytes(arr));
        return t.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    }
    format!("unparsed:{text}")
}

pub fn short_hash(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(data);
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- diff

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// in the first document only
    Missing(String),
    /// in the second document only
    Added(String),
    Changed(String, String),
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub path: String,
    pub change: Change,
}

/// Compare two documents. When a whole element disappears, only the element
/// itself is reported, not every record below it.
pub fn diff(before: &Canon, after: &Canon) -> Vec<Diff> {
    let mut out = Vec::new();
    let mut missing_roots: BTreeSet<String> = BTreeSet::new();
    let mut added_roots: BTreeSet<String> = BTreeSet::new();

    for (path, old) in &before.records {
        match after.records.get(path) {
            Some(new) if new == old => {}
            Some(new) => out.push(Diff {
                path: path.clone(),
                change: Change::Changed(old.clone(), new.clone()),
            }),
            None => {
                if !has_ancestor(&missing_roots, path) {
                    if old == EXISTS {
                        missing_roots.insert(path.clone());
                    }
                    out.push(Diff {
                        path: path.clone(),
                        change: Change::Missing(old.clone()),
                    });
                }
            }
        }
    }
    for (path, new) in &after.records {
        if !before.records.contains_key(path) && !has_ancestor(&added_roots, path) {
            if new == EXISTS {
                added_roots.insert(path.clone());
            }
            out.push(Diff {
                path: path.clone(),
                change: Change::Added(new.clone()),
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn has_ancestor(roots: &BTreeSet<String>, path: &str) -> bool {
    roots.iter().any(|r| {
        path.len() > r.len() && path.starts_with(r.as_str()) && path.as_bytes()[r.len()] == b'/'
    })
}

/// Splits a path on '/', except inside [...] (Base64 UUIDs contain '/').
pub fn segments(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in path.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            '/' if depth == 0 => {
                out.push(&path[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&path[start..]);
    out
}

/// Make a path readable by replacing UUID segments with group / entry names.
pub fn display_path(path: &str, labels: &[&HashMap<String, String>]) -> String {
    segments(path)
        .into_iter()
        .map(|seg| {
            labels
                .iter()
                .find_map(|l| l.get(seg))
                .cloned()
                .unwrap_or_else(|| seg.to_string())
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<KeePassFile><Meta><Generator>x</Generator><DatabaseName>db</DatabaseName></Meta>
      <Root><Group><UUID>AQ==</UUID><Name>Root</Name><Tags>a,b</Tags>
        <Entry><UUID>Ag==</UUID><String><Key>Title</Key><Value>T</Value></String>
          <String><Key>Password</Key><Value Protected="True">c2VjcmV0</Value></String>
          <Times><CreationTime>2024-01-02T03:04:05Z</CreationTime></Times>
        </Entry></Group></Root></KeePassFile>"#;

    #[test]
    fn flattens_with_stable_keys() {
        let c = parse(DOC, Source::Raw).unwrap();
        assert_eq!(c.records["Meta/DatabaseName"], "db");
        assert!(!c.records.contains_key("Meta/Generator"));
        assert_eq!(c.records["Root/Group[AQ==]/Tags"], "a;b");
        assert_eq!(
            c.records["Root/Group[AQ==]/Entry[Ag==]/String[Title]/Value"],
            "T"
        );
        assert_eq!(
            c.records["Root/Group[AQ==]/Entry[Ag==]/String[Password]/Value"],
            PROTECTED
        );
        assert_eq!(
            c.records["Root/Group[AQ==]/Entry[Ag==]/Times/CreationTime"],
            "2024-01-02T03:04:05Z"
        );
        assert_eq!(c.records["Root/Group[AQ==]/@entries"], "Entry[Ag==]");
    }

    #[test]
    fn base64_and_iso_times_compare_equal() {
        // 2024-01-02T03:04:05Z as KDBX 4 seconds since 0001-01-01
        let secs: i64 =
            (NaiveDateTime::parse_from_str("2024-01-02T03:04:05Z", "%Y-%m-%dT%H:%M:%SZ").unwrap()
                - epoch())
            .num_seconds();
        assert_eq!(
            normalize_time(&B64.encode(secs.to_le_bytes())),
            "2024-01-02T03:04:05Z"
        );
    }

    #[test]
    fn diff_collapses_removed_subtrees() {
        let a = parse(DOC, Source::Raw).unwrap();
        let b = parse(
            &DOC.replace("<Entry><UUID>Ag==</UUID>", "<Entry><UUID>Aw==</UUID>"),
            Source::Raw,
        )
        .unwrap();
        let d = diff(&a, &b);
        let missing: Vec<_> = d
            .iter()
            .filter(|d| matches!(d.change, Change::Missing(_)))
            .collect();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].path, "Root/Group[AQ==]/Entry[Ag==]");
    }
}
