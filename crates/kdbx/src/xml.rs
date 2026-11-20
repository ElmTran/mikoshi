//! A KeePass XML document as a tree that keeps everything: element order,
//! attributes, whitespace, comments, elements nobody knows. Values marked
//! `Protected="True"` are decrypted while reading (in document order, as the
//! inner stream requires) and kept in memory that is wiped on drop.

use std::borrow::Cow;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use zeroize::Zeroizing;

use crate::crypto::InnerStream;
use crate::error::{Error, Result};

/// Deeper nesting is refused: the tree is walked recursively.
const MAX_DEPTH: usize = 256;

const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

#[derive(Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
    /// the plaintext of an element marked `Protected="True"`
    Secret(Zeroizing<Vec<u8>>),
    Comment(String),
    Pi(String, Option<String>),
}

#[derive(Clone, PartialEq, Eq, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl std::fmt::Debug for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // never print values
        write!(f, "<{}> ({} children)", self.name, self.children.len())
    }
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Node::Element(e) => e.fmt(f),
            Node::Text(_) => f.write_str("text"),
            Node::Secret(_) => f.write_str("secret"),
            Node::Comment(_) => f.write_str("comment"),
            Node::Pi(..) => f.write_str("processing instruction"),
        }
    }
}

pub fn is_true(v: &str) -> bool {
    v.trim().eq_ignore_ascii_case("true")
}

impl Element {
    pub fn new(name: &str) -> Element {
        Element {
            name: name.to_string(),
            ..Element::default()
        }
    }

    /// A leaf element holding `text`.
    pub fn with_text(name: &str, text: &str) -> Element {
        let mut e = Element::new(name);
        e.set_text(text);
        e
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, name: &str, value: &str) {
        match self.attrs.iter_mut().find(|(k, _)| k == name) {
            Some(a) => a.1 = value.to_string(),
            None => self.attrs.push((name.to_string(), value.to_string())),
        }
    }

    pub fn remove_attr(&mut self, name: &str) {
        self.attrs.retain(|(k, _)| k != name);
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    pub fn elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|c| match c {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.elements_mut().find(|e| e.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements().filter(move |e| e.name == name)
    }

    /// The text content (all text children joined). Empty for a protected value.
    pub fn text(&self) -> Cow<'_, str> {
        let mut texts = self.children.iter().filter_map(|c| match c {
            Node::Text(t) => Some(t.as_str()),
            _ => None,
        });
        match (texts.next(), texts.next()) {
            (None, _) => Cow::Borrowed(""),
            (Some(t), None) => Cow::Borrowed(t),
            (Some(a), Some(b)) => {
                let mut s = String::from(a);
                s.push_str(b);
                texts.for_each(|t| s.push_str(t));
                Cow::Owned(s)
            }
        }
    }

    /// The text of a child element, if there is such a child.
    pub fn child_text(&self, name: &str) -> Option<Cow<'_, str>> {
        self.child(name).map(Element::text)
    }

    /// The decrypted value when the element is protected.
    pub fn secret(&self) -> Option<&[u8]> {
        self.children.iter().find_map(|c| match c {
            Node::Secret(s) => Some(s.as_slice()),
            _ => None,
        })
    }

    pub fn is_protected(&self) -> bool {
        self.attr("Protected").is_some_and(is_true)
    }

    fn clear_value(&mut self) {
        self.children
            .retain(|c| !matches!(c, Node::Text(_) | Node::Secret(_)));
    }

    /// Replaces the text content; the element stays unprotected.
    pub fn set_text(&mut self, text: &str) {
        self.clear_value();
        self.remove_attr("Protected");
        if !text.is_empty() {
            self.children.insert(0, Node::Text(text.to_string()));
        }
    }

    /// Replaces the content with a value encrypted by the inner stream on save.
    pub fn set_secret(&mut self, data: &[u8]) {
        self.clear_value();
        self.set_attr("Protected", "True");
        self.children
            .insert(0, Node::Secret(Zeroizing::new(data.to_vec())));
    }

    /// Inserts `el` right after the last child element named in `after`;
    /// failing that, right before the first one named in `before`; failing
    /// that, at the end. Indents it like its siblings when they are indented.
    pub fn insert_element(&mut self, mut el: Element, after: &[&str], before: &[&str]) {
        let is = |c: &Node, names: &[&str]| matches!(c, Node::Element(e) if names.contains(&e.name.as_str()));
        let elems: Vec<usize> = (0..self.children.len())
            .filter(|&i| matches!(self.children[i], Node::Element(_)))
            .collect();
        // whitespace between siblings, e.g. "\n\t\t\t"
        let indent =
            elems
                .first()
                .and_then(|&i| i.checked_sub(1))
                .and_then(|i| match &self.children[i] {
                    Node::Text(t) if t.trim().is_empty() && t.contains('\n') => Some(t.clone()),
                    _ => None,
                });
        let pos = if let Some(i) = (0..self.children.len())
            .rev()
            .find(|&i| is(&self.children[i], after))
        {
            i + 1
        } else if let Some(i) = (0..self.children.len()).find(|&i| is(&self.children[i], before)) {
            // keep the indentation in front of `before` where it is
            match i.checked_sub(1).map(|j| &self.children[j]) {
                Some(Node::Text(t)) if t.trim().is_empty() => i - 1,
                _ => i,
            }
        } else {
            match self.children.last() {
                Some(Node::Text(t)) if t.trim().is_empty() && !elems.is_empty() => {
                    self.children.len() - 1
                }
                _ => self.children.len(),
            }
        };
        match indent {
            Some(ind) => {
                let unit = indent_unit(&ind);
                indent_children(&mut el, &ind, unit);
                self.children
                    .splice(pos..pos, [Node::Text(ind), Node::Element(el)]);
            }
            None => self.children.insert(pos, Node::Element(el)),
        }
    }

    /// Removes the first child element named `name`, with the whitespace in
    /// front of it, and returns it.
    pub fn remove_child(&mut self, name: &str) -> Option<Element> {
        let i = self
            .children
            .iter()
            .position(|c| matches!(c, Node::Element(e) if e.name == name))?;
        self.remove_at(i)
    }

    /// Removes the child node at `i` (an element) and the whitespace in front.
    pub fn remove_at(&mut self, i: usize) -> Option<Element> {
        let Node::Element(_) = self.children.get(i)? else {
            return None;
        };
        let Node::Element(el) = self.children.remove(i) else {
            return None;
        };
        if let Some(Node::Text(t)) = i.checked_sub(1).and_then(|j| self.children.get(j))
            && t.trim().is_empty()
        {
            self.children.remove(i - 1);
        }
        Some(el)
    }
}

/// Re-indents the inside of `el` for an element whose closing tag sits after
/// the whitespace `template`'s last child holds (a newline and tabs, say).
/// Does nothing when `template` is written without indentation. Leaf values
/// are not touched, whitespace-only ones included.
pub(crate) fn reindent_like(el: &mut Element, template: &Element) {
    if let Some(Node::Text(outer)) = template.children.last()
        && outer.trim().is_empty()
        && outer.contains('\n')
    {
        let outer = outer.clone();
        indent_children(el, &outer, indent_unit(&outer));
    }
}

fn indent_unit(indent: &str) -> &'static str {
    if indent.ends_with(' ') { "  " } else { "\t" }
}

/// Indents the inside of a new element whose own line starts with `outer`.
fn indent_children(el: &mut Element, outer: &str, unit: &str) {
    if el.elements().next().is_none() {
        return;
    }
    let inner = format!("{outer}{unit}");
    let old = std::mem::take(&mut el.children);
    for mut c in old {
        if let Node::Text(t) = &c
            && t.trim().is_empty()
        {
            continue;
        }
        if let Node::Element(e) = &mut c {
            indent_children(e, &inner, unit);
        }
        el.children.push(Node::Text(inner.clone()));
        el.children.push(c);
    }
    el.children.push(Node::Text(outer.to_string()));
}

// ---------------------------------------------------------------- reading

pub fn parse(xml: &str, stream: &mut InnerStream) -> Result<Element> {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: u32::MAX,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(xml.trim_start_matches('\u{feff}'), opts)
        .map_err(|_| Error::Corrupt("the XML inside cannot be parsed"))?;
    convert(doc.root_element(), stream, 0)
}

fn convert(
    node: roxmltree::Node<'_, '_>,
    stream: &mut InnerStream,
    depth: usize,
) -> Result<Element> {
    if depth > MAX_DEPTH {
        return Err(Error::Corrupt("the XML is nested too deeply"));
    }
    let mut el = Element {
        name: node.tag_name().name().to_string(),
        attrs: node
            .attributes()
            .map(|a| {
                let name = match a.namespace() {
                    Some(XML_NS) => format!("xml:{}", a.name()),
                    _ => a.name().to_string(),
                };
                (name, a.value().to_string())
            })
            .collect(),
        children: Vec::new(),
    };
    let protected = el.is_protected() && !node.children().any(|c| c.is_element());
    if protected {
        let text: String = node.children().filter_map(|c| c.text()).collect::<String>();
        let clean: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        let mut data = Zeroizing::new(
            B64.decode(clean)
                .map_err(|_| Error::Corrupt("a protected value is not Base64"))?,
        );
        stream.apply(&mut data);
        // comments inside a protected value are dropped (KeePass never writes any)
        el.children.push(Node::Secret(data));
        return Ok(el);
    }
    for c in node.children() {
        match c.node_type() {
            roxmltree::NodeType::Element => {
                el.children
                    .push(Node::Element(convert(c, stream, depth + 1)?));
            }
            roxmltree::NodeType::Text => {
                let t = c.text().unwrap_or("");
                // roxmltree may split text around entities; join adjacent runs
                if let Some(Node::Text(prev)) = el.children.last_mut() {
                    prev.push_str(t);
                } else {
                    el.children.push(Node::Text(t.to_string()));
                }
            }
            roxmltree::NodeType::Comment => {
                el.children
                    .push(Node::Comment(c.text().unwrap_or("").to_string()));
            }
            roxmltree::NodeType::PI => {
                if let Some(pi) = c.pi() {
                    el.children.push(Node::Pi(
                        pi.target.to_string(),
                        pi.value.map(str::to_string),
                    ));
                }
            }
            roxmltree::NodeType::Root => {}
        }
    }
    Ok(el)
}

// ---------------------------------------------------------------- writing

pub const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\"?>\n";

/// Serializes the document; protected values are encrypted with `stream` in
/// document order.
pub fn write(root: &Element, stream: &mut InnerStream) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(String::with_capacity(64 * 1024));
    out.push_str(DECLARATION);
    write_element(root, stream, &mut out);
    // move the bytes out without leaving a copy behind
    let s = std::mem::take(&mut *out);
    Zeroizing::new(s.into_bytes())
}

fn allowed(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

fn escape_into(out: &mut String, s: &str, attr: bool) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            '"' if attr => out.push_str("&quot;"),
            '\n' if attr => out.push_str("&#xA;"),
            '\t' if attr => out.push_str("&#x9;"),
            // characters XML cannot carry at all; KeePass drops them too
            c if !allowed(c) => {}
            c => out.push(c),
        }
    }
}

fn write_element(el: &Element, stream: &mut InnerStream, out: &mut String) {
    out.push('<');
    out.push_str(&el.name);
    for (k, v) in &el.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape_into(out, v, true);
        out.push('"');
    }
    if el.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in &el.children {
        match c {
            Node::Element(e) => write_element(e, stream, out),
            Node::Text(t) => escape_into(out, t, false),
            Node::Secret(s) => {
                let mut data = Zeroizing::new(s.to_vec());
                stream.apply(&mut data);
                out.push_str(&B64.encode(&data[..]));
            }
            Node::Comment(t) => {
                out.push_str("<!--");
                out.push_str(&t.replace("--", "- -"));
                out.push_str("-->");
            }
            Node::Pi(target, value) => {
                out.push_str("<?");
                out.push_str(target);
                if let Some(v) = value {
                    out.push(' ');
                    out.push_str(&v.replace("?>", "? >"));
                }
                out.push_str("?>");
            }
        }
    }
    out.push_str("</");
    out.push_str(&el.name);
    out.push('>');
}

/// Writes the document with protected values in plain text, marked
/// `ProtectInMemory="True"` the way KeePass marks them in an XML export.
/// For comparing with other programs' exports; never written to disk by the app.
pub fn write_plain(root: &Element) -> Zeroizing<String> {
    let mut copy = root.clone();
    fn unprotect(el: &mut Element) {
        if let Some(s) = el.secret() {
            let text = String::from_utf8_lossy(s).into_owned();
            let text = Zeroizing::new(text);
            el.clear_value();
            el.remove_attr("Protected");
            el.set_attr("ProtectInMemory", "True");
            if !text.is_empty() {
                el.children.insert(0, Node::Text(text.to_string()));
            }
        }
        el.elements_mut().for_each(unprotect);
    }
    unprotect(&mut copy);
    let mut out = Zeroizing::new(String::new());
    out.push_str(DECLARATION);
    write_element(&copy, &mut InnerStream::None, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(xml: &str) -> String {
        let el = parse(xml, &mut InnerStream::None).unwrap();
        let out = write(&el, &mut InnerStream::None);
        String::from_utf8(out.to_vec()).unwrap()
    }

    #[test]
    fn keeps_whitespace_order_comments_and_unknown_elements() {
        let xml = "<A x=\"1\" b=\"2\">\n\t<Z>  </Z>\n\t<!-- c -->\n\t<Unknown k=\"v\"><Q/></Unknown>\n\t<Y>a &amp; b &lt;c&gt; \"q\"</Y>\n</A>";
        let out = round(xml);
        assert_eq!(out, format!("{DECLARATION}{xml}"));
    }

    #[test]
    fn escapes_carriage_returns_and_attribute_whitespace() {
        let mut el = Element::new("A");
        el.set_attr("v", "a\"b\nc\td");
        el.set_text("line\r\nnext");
        let mut s = String::new();
        write_element(&el, &mut InnerStream::None, &mut s);
        assert_eq!(s, "<A v=\"a&quot;b&#xA;c&#x9;d\">line&#xD;\nnext</A>");
        let back = parse(&s, &mut InnerStream::None).unwrap();
        assert_eq!(back.text(), "line\r\nnext");
        assert_eq!(back.attr("v"), Some("a\"b\nc\td"));
    }

    #[test]
    fn drops_characters_xml_cannot_hold() {
        let mut s = String::new();
        write_element(
            &Element::with_text("A", "a\u{1}b\u{FFFF}"),
            &mut InnerStream::None,
            &mut s,
        );
        assert_eq!(s, "<A>ab</A>");
    }

    #[test]
    fn refuses_deep_nesting() {
        let xml = "<a>".repeat(MAX_DEPTH + 2) + &"</a>".repeat(MAX_DEPTH + 2);
        assert!(parse(&xml, &mut InnerStream::None).is_err());
    }

    #[test]
    fn inserts_with_the_siblings_indentation() {
        let xml = "<G>\n\t<UUID>u</UUID>\n\t<Entry/>\n\t<Group/>\n</G>";
        let mut g = parse(xml, &mut InnerStream::None).unwrap();
        let mut e = Element::new("Entry");
        e.children
            .push(Node::Element(Element::with_text("UUID", "x")));
        g.insert_element(e, &["Entry"], &["Group"]);
        let mut s = String::new();
        write_element(&g, &mut InnerStream::None, &mut s);
        assert_eq!(
            s,
            "<G>\n\t<UUID>u</UUID>\n\t<Entry/>\n\t<Entry>\n\t\t<UUID>x</UUID>\n\t</Entry>\n\t<Group/>\n</G>"
        );
        let removed = g.remove_child("Group").unwrap();
        assert_eq!(removed.name, "Group");
        let mut s = String::new();
        write_element(&g, &mut InnerStream::None, &mut s);
        assert!(s.ends_with("</Entry>\n</G>"));
    }
}
