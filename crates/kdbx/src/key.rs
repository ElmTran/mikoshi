//! The composite key: password, key file, or both.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use zeroize::Zeroizing;

use crate::crypto::sha256;
use crate::error::{Error, Result};

/// What unlocks a database. Holds only hashes of the password and the key
/// file's key, wiped on drop.
#[derive(Default, Clone)]
pub struct Key {
    password: Option<Zeroizing<[u8; 32]>>,
    keyfile: Option<Zeroizing<Vec<u8>>>,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Key")
            .field("password", &self.password.is_some())
            .field("keyfile", &self.keyfile.is_some())
            .finish()
    }
}

impl Key {
    pub fn new() -> Key {
        Key::default()
    }

    /// An empty password still counts as a password, as in KeePass. Callers
    /// that treat "" as "no password" leave it out.
    pub fn with_password(mut self, password: &str) -> Key {
        self.password = Some(Zeroizing::new(sha256(&[password.as_bytes()])));
        self
    }

    /// The contents of a key file, in any of the formats KeePass accepts:
    /// XML 1.0, XML 2.0, 32 raw bytes, 64 hex digits, or anything else (hashed).
    pub fn with_keyfile(mut self, contents: &[u8]) -> Result<Key> {
        self.keyfile = Some(keyfile_key(contents)?);
        Ok(self)
    }

    pub fn is_empty(&self) -> bool {
        self.password.is_none() && self.keyfile.is_none()
    }

    pub fn has_password(&self) -> bool {
        self.password.is_some()
    }

    pub fn has_keyfile(&self) -> bool {
        self.keyfile.is_some()
    }

    /// This key with the password part of `other` (or none, if it has none),
    /// for changing one part of a key and keeping the other.
    pub fn with_password_of(mut self, other: &Key) -> Key {
        self.password = other.password.clone();
        self
    }

    /// This key with the key file part of `other` (or none).
    pub fn with_keyfile_of(mut self, other: &Key) -> Key {
        self.keyfile = other.keyfile.clone();
        self
    }

    /// Whether the password part is the same as `other`'s (both absent counts).
    /// Compares in constant time.
    pub fn same_password(&self, other: &Key) -> bool {
        match (&self.password, &other.password) {
            (Some(a), Some(b)) => same_bytes(&a[..], &b[..]),
            (None, None) => true,
            _ => false,
        }
    }

    /// Whether the key file part is the same as `other`'s (both absent counts).
    pub fn same_keyfile(&self, other: &Key) -> bool {
        match (&self.keyfile, &other.keyfile) {
            (Some(a), Some(b)) => same_bytes(a, b),
            (None, None) => true,
            _ => false,
        }
    }

    pub(crate) fn composite(&self) -> Result<Zeroizing<[u8; 32]>> {
        let mut parts: Vec<&[u8]> = Vec::new();
        if let Some(p) = &self.password {
            parts.push(&p[..]);
        }
        if let Some(k) = &self.keyfile {
            parts.push(&k[..]);
        }
        if parts.is_empty() {
            return Err(Error::NoKey);
        }
        Ok(Zeroizing::new(sha256(&parts)))
    }
}

/// Equal bytes, without stopping at the first difference.
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn keyfile_key(data: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if let Some(k) = xml_keyfile(data)? {
        return Ok(k);
    }
    if data.len() == 32 {
        return Ok(Zeroizing::new(data.to_vec()));
    }
    if data.len() == 64
        && let Some(k) = hex(data, false)
    {
        return Ok(k);
    }
    Ok(Zeroizing::new(sha256(&[data]).to_vec()))
}

/// `spaced`: whitespace between the digits is allowed (XML 2.0 key files).
fn hex(data: &[u8], spaced: bool) -> Option<Zeroizing<Vec<u8>>> {
    let digits: Vec<u8> = data
        .iter()
        .copied()
        .filter(|b| !(spaced && b.is_ascii_whitespace()))
        .collect();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    let val = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
    let mut out = Zeroizing::new(Vec::with_capacity(digits.len() / 2));
    for pair in digits.chunks(2) {
        out.push(val(pair[0])? << 4 | val(pair[1])?);
    }
    Some(out)
}

/// `Ok(None)` when the file is not a KeePass XML key file, which makes it an
/// ordinary file whose hash is the key (as in KeePass).
fn xml_keyfile(data: &[u8]) -> Result<Option<Zeroizing<Vec<u8>>>> {
    let Ok(text) = std::str::from_utf8(data) else {
        return Ok(None);
    };
    let text = text.trim_start_matches('\u{feff}');
    let Ok(doc) = roxmltree::Document::parse(text) else {
        return Ok(None);
    };
    let root = doc.root_element();
    if !root.has_tag_name("KeyFile") {
        return Ok(None);
    }
    fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
        n.children()
            .find(|c| c.is_element() && c.has_tag_name(name))
    }
    let version = child(root, "Meta")
        .and_then(|m| child(m, "Version"))
        .and_then(|v| v.text())
        .map(str::trim)
        .unwrap_or("");
    let Some(data_el) = child(root, "Key").and_then(|k| child(k, "Data")) else {
        return Ok(None);
    };
    let body = data_el.text().unwrap_or("");
    if version.starts_with("1.") {
        let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        let key = B64
            .decode(clean)
            .map_err(|_| Error::KeyFile("bad Base64 data"))?;
        return Ok(Some(Zeroizing::new(key)));
    }
    if version.starts_with("2.") {
        let key = hex(body.as_bytes(), true).ok_or(Error::KeyFile("bad hex data"))?;
        if let Some(h) = data_el.attribute("Hash") {
            let want = hex(h.as_bytes(), true).ok_or(Error::KeyFile("bad hash"))?;
            if sha256(&[&key]).get(..want.len()) != Some(&want[..]) {
                return Err(Error::KeyFile("the key does not match its hash"));
            }
        }
        return Ok(Some(key));
    }
    Err(Error::KeyFile("unsupported key file version"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_or_odd_files_are_hashed() {
        let k = keyfile_key(b"hello").unwrap();
        assert_eq!(&k[..], &sha256(&[b"hello"])[..]);
        let raw = [7u8; 32];
        assert_eq!(&keyfile_key(&raw).unwrap()[..], &raw[..]);
        let hex64 = "ab".repeat(32);
        assert_eq!(&keyfile_key(hex64.as_bytes()).unwrap()[..], &[0xab; 32][..]);
        // 64 bytes that are not hex: hashed
        let nothex = "zz".repeat(32);
        assert_eq!(
            &keyfile_key(nothex.as_bytes()).unwrap()[..],
            &sha256(&[nothex.as_bytes()])[..]
        );
    }

    #[test]
    fn parts_can_be_kept_and_compared() {
        let both = Key::new()
            .with_password("pw")
            .with_keyfile(b"file")
            .unwrap();
        let other_pw = Key::new().with_password("other").with_keyfile_of(&both);
        assert!(other_pw.same_keyfile(&both));
        assert!(!other_pw.same_password(&both));
        assert!(other_pw.has_password() && other_pw.has_keyfile());

        let file_only = Key::new().with_keyfile_of(&both);
        assert!(!file_only.has_password());
        assert!(!file_only.same_password(&both));
        assert!(
            Key::new().same_password(&file_only),
            "both without a password"
        );

        let pw_again = Key::new().with_password("pw");
        assert!(pw_again.same_password(&both));
        assert_eq!(
            Key::new()
                .with_password_of(&both)
                .with_keyfile_of(&both)
                .composite()
                .unwrap()[..],
            both.composite().unwrap()[..]
        );
    }

    #[test]
    fn xml_v2_checks_its_hash() {
        let key = [0x11u8; 32];
        let hash = sha256(&[&key]);
        let hx = |b: &[u8]| b.iter().map(|x| format!("{x:02X}")).collect::<String>();
        let doc = |h: &str| {
            format!(
                "<?xml version=\"1.0\"?><KeyFile><Meta><Version>2.0</Version></Meta><Key><Data Hash=\"{h}\">{}</Data></Key></KeyFile>",
                hx(&key)
            )
        };
        assert_eq!(
            &keyfile_key(doc(&hx(&hash[..4])).as_bytes()).unwrap()[..],
            &key[..]
        );
        assert_eq!(
            keyfile_key(doc("00000000").as_bytes()).unwrap_err(),
            Error::KeyFile("the key does not match its hash")
        );
    }
}
