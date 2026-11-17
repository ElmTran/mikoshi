//! Minimal KDBX 4.1 writer: plaintext KeePass XML in, encrypted file out.
//!
//! Only used to build test files that keepassxc-cli cannot make, because
//! KeePassXC drops the elements under test when it imports XML. Fixed
//! settings: AES-256, AES-KDF (few rounds), GZip, ChaCha20 inner stream.
//! Every file it writes is also opened with keepassxc-cli as a sanity check.

use std::io::{Read, Write};

use aes::Aes256;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use cipher::block_padding::Pkcs7;
use cipher::{BlockCipherEncrypt, BlockModeEncrypt, KeyInit, KeyIvInit, StreamCipher};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};

/// Extra XML to insert just before the closing tag of the matching element.
pub struct Injection {
    pub target: Target,
    pub xml: &'static str,
}

pub enum Target {
    Meta,
    Root,
    /// a group, by name
    Group(&'static str),
    /// a current (non-history) entry, by title
    Entry(&'static str),
    /// the AutoType element of an entry, by title
    AutoType(&'static str),
}

const AES_KDF: [u8; 16] = [
    0xc9, 0xd9, 0xf3, 0x9a, 0x62, 0x8a, 0x44, 0x60, 0xbf, 0x74, 0x0d, 0x08, 0xc1, 0x8a, 0x4f, 0xea,
];
const AES_256: [u8; 16] = [
    0x31, 0xc1, 0xf2, 0xe6, 0xbf, 0x71, 0x43, 0x50, 0xbe, 0x58, 0x05, 0x21, 0x6a, 0xfc, 0x5a, 0xff,
];
const ROUNDS: u64 = 1000;

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

fn sha256(parts: &[&[u8]]) -> Vec<u8> {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().to_vec()
}

fn sha512(parts: &[&[u8]]) -> Vec<u8> {
    let mut h = Sha512::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().to_vec()
}

fn hmac256(key: &[u8], parts: &[&[u8]]) -> Result<Vec<u8>, String> {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).map_err(|e| e.to_string())?;
    for p in parts {
        m.update(p);
    }
    Ok(m.finalize().into_bytes().to_vec())
}

pub fn write(xml: &str, password: &str, injections: &[Injection]) -> Result<Vec<u8>, String> {
    // ---- inner XML: protected values encrypted in document order, attachments
    // moved from Meta/Binaries to the inner header
    let inner_key: [u8; 64] = random()?;
    let stream_key = sha512(&[&inner_key]);
    let mut stream = chacha20::ChaCha20::new_from_slices(&stream_key[..32], &stream_key[32..44])
        .map_err(|e| e.to_string())?;
    let doc = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: u32::MAX,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let mut ser = Serializer {
        out: String::new(),
        stream: &mut stream,
        binaries: Vec::new(),
        injections,
    };
    ser.out
        .push_str("<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\"?>\n");
    ser.element(doc.root_element())?;
    let Serializer {
        out: inner_xml,
        mut binaries,
        ..
    } = ser;
    binaries.sort_by_key(|(id, _)| *id);

    let mut payload = Vec::new();
    push_field(&mut payload, 1, &3u32.to_le_bytes());
    push_field(&mut payload, 2, &inner_key);
    for (_, data) in &binaries {
        let mut body = vec![0u8];
        body.extend_from_slice(data);
        push_field(&mut payload, 3, &body);
    }
    push_field(&mut payload, 0, &[]);
    payload.extend_from_slice(inner_xml.as_bytes());

    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&payload).map_err(|e| e.to_string())?;
    let compressed = gz.finish().map_err(|e| e.to_string())?;

    // ---- keys
    let master_seed: [u8; 32] = random()?;
    let iv: [u8; 16] = random()?;
    let kdf_seed: [u8; 32] = random()?;
    let composite = sha256(&[&sha256(&[password.as_bytes()])]);
    let transformed = aes_kdf(&composite, &kdf_seed, ROUNDS)?;
    let master_key = sha256(&[&master_seed, &transformed]);
    let hmac_base = sha512(&[&master_seed, &transformed, &[1]]);

    // ---- outer header
    let mut header = vec![
        0x03, 0xd9, 0xa2, 0x9a, 0x67, 0xfb, 0x4b, 0xb5, 0x01, 0x00, 0x04, 0x00,
    ];
    push_field(&mut header, 2, &AES_256);
    push_field(&mut header, 3, &1u32.to_le_bytes());
    push_field(&mut header, 4, &master_seed);
    push_field(&mut header, 7, &iv);
    let mut vd = vec![0x00, 0x01];
    push_vd(&mut vd, 0x42, "$UUID", &AES_KDF);
    push_vd(&mut vd, 0x05, "R", &ROUNDS.to_le_bytes());
    push_vd(&mut vd, 0x42, "S", &kdf_seed);
    vd.push(0);
    push_field(&mut header, 11, &vd);
    push_field(&mut header, 0, b"\r\n\r\n");

    let mut out = header.clone();
    out.extend_from_slice(&sha256(&[&header]));
    out.extend_from_slice(&hmac256(
        &sha512(&[&u64::MAX.to_le_bytes(), &hmac_base]),
        &[&header],
    )?);

    // ---- payload: AES-256-CBC, then HMAC block stream
    let encrypted = cbc::Encryptor::<Aes256>::new_from_slices(&master_key, &iv)
        .map_err(|e| e.to_string())?
        .encrypt_padded_vec::<Pkcs7>(&compressed);
    for (index, block) in [(0u64, encrypted.as_slice()), (1u64, &[][..])] {
        let len = (block.len() as u32).to_le_bytes();
        let key = sha512(&[&index.to_le_bytes(), &hmac_base]);
        out.extend_from_slice(&hmac256(&key, &[&index.to_le_bytes(), &len, block])?);
        out.extend_from_slice(&len);
        out.extend_from_slice(block);
    }
    Ok(out)
}

fn push_field(out: &mut Vec<u8>, kind: u8, body: &[u8]) {
    out.push(kind);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
}

fn push_vd(out: &mut Vec<u8>, kind: u8, key: &str, value: &[u8]) {
    out.push(kind);
    out.extend_from_slice(&(key.len() as u32).to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value);
}

fn aes_kdf(composite: &[u8], seed: &[u8], rounds: u64) -> Result<Vec<u8>, String> {
    let cipher = Aes256::new_from_slice(seed).map_err(|e| e.to_string())?;
    let mut a = aes::Block::try_from(&composite[..16]).map_err(|_| "bad key length")?;
    let mut b = aes::Block::try_from(&composite[16..32]).map_err(|_| "bad key length")?;
    for _ in 0..rounds {
        cipher.encrypt_block(&mut a);
        cipher.encrypt_block(&mut b);
    }
    Ok(sha256(&[&a, &b]))
}

fn escape(s: &str, attr: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

struct Serializer<'a> {
    out: String,
    stream: &'a mut chacha20::ChaCha20,
    binaries: Vec<(usize, Vec<u8>)>,
    injections: &'a [Injection],
}

fn child_text<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|c| c.has_tag_name(name))
        .and_then(|c| c.text())
}

fn entry_title<'a>(node: roxmltree::Node<'a, 'a>) -> Option<&'a str> {
    node.children()
        .filter(|c| c.has_tag_name("String"))
        .find(|s| child_text(*s, "Key") == Some("Title"))
        .and_then(|s| child_text(s, "Value"))
}

impl Serializer<'_> {
    fn matches(&self, node: roxmltree::Node<'_, '_>, target: &Target) -> bool {
        let name = node.tag_name().name();
        let parent = node
            .parent_element()
            .map(|p| p.tag_name().name())
            .unwrap_or("");
        match target {
            Target::Meta => name == "Meta",
            Target::Root => name == "Root",
            Target::Group(n) => name == "Group" && child_text(node, "Name") == Some(n),
            Target::Entry(t) => {
                name == "Entry" && parent == "Group" && entry_title(node) == Some(t)
            }
            Target::AutoType(t) => {
                name == "AutoType"
                    && node.parent_element().is_some_and(|e| {
                        e.parent_element().is_some_and(|g| g.has_tag_name("Group"))
                            && entry_title(e) == Some(t)
                    })
            }
        }
    }

    fn element(&mut self, node: roxmltree::Node<'_, '_>) -> Result<(), String> {
        let name = node.tag_name().name();
        let parent = node
            .parent_element()
            .map(|p| p.tag_name().name())
            .unwrap_or("");
        if name == "Binaries" && parent == "Meta" {
            for b in node.children().filter(|c| c.has_tag_name("Binary")) {
                let id: usize = b
                    .attribute("ID")
                    .and_then(|v| v.parse().ok())
                    .ok_or("Binary without ID")?;
                let mut data = B64
                    .decode(b.text().unwrap_or("").trim())
                    .map_err(|e| e.to_string())?;
                if b.attribute("Compressed")
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"))
                    && !data.is_empty()
                {
                    let mut plain = Vec::new();
                    flate2::read::GzDecoder::new(&data[..])
                        .read_to_end(&mut plain)
                        .map_err(|e| e.to_string())?;
                    data = plain;
                }
                self.binaries.push((id, data));
            }
            return Ok(());
        }

        let protect = name == "Value"
            && parent == "String"
            && ["Protected", "ProtectInMemory"].iter().any(|a| {
                node.attribute(*a)
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"))
            });

        self.out.push('<');
        self.out.push_str(name);
        for a in node.attributes() {
            if protect && (a.name() == "Protected" || a.name() == "ProtectInMemory") {
                continue;
            }
            self.out
                .push_str(&format!(" {}=\"{}\"", a.name(), escape(a.value(), true)));
        }
        if protect {
            self.out.push_str(" Protected=\"True\"");
        }
        let injected: Vec<&str> = self
            .injections
            .iter()
            .filter(|i| self.matches(node, &i.target))
            .map(|i| i.xml)
            .collect();
        if !node.has_children() && injected.is_empty() {
            self.out.push_str("/>");
            return Ok(());
        }
        self.out.push('>');
        if protect {
            let mut data = node.text().unwrap_or("").as_bytes().to_vec();
            self.stream.apply_keystream(&mut data);
            self.out.push_str(&B64.encode(&data));
        } else {
            for child in node.children() {
                if child.is_element() {
                    self.element(child)?;
                } else if let Some(t) = child.text() {
                    self.out.push_str(&escape(t, false));
                }
            }
        }
        for x in injected {
            self.out.push_str(x);
        }
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push('>');
        Ok(())
    }
}
