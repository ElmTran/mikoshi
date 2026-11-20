//! Opening and saving: the encrypted container around the XML tree.

use std::collections::HashMap;
use std::io::{Read, Write};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use zeroize::Zeroizing;

use crate::crypto::{self, Cipher, InnerStream, STREAM_CHACHA20, STREAM_SALSA20, sha256, sha512};
use crate::error::{Error, Result};
use crate::header::{
    Binary, COMPRESSION_GZIP, COMPRESSION_NONE, InnerHeader, Kdf, OuterHeader, Reader, VD_BYTES,
    transform_key,
};
use crate::key::Key;
use crate::types::Timestamp;
use crate::xml::{self, Element, Node, is_true};

/// HMAC blocks KeePass writes: 1 MiB each.
const BLOCK_SIZE: usize = 1 << 20;

/// Time elements KeePass writes. KDBX 4 readers (KeePass 2 among them)
/// only understand the Base64 form, so 3.1 files are converted on open.
pub const TIME_ELEMENTS: &[&str] = &[
    "CreationTime",
    "LastModificationTime",
    "LastAccessTime",
    "ExpiryTime",
    "LocationChanged",
    "DeletionTime",
    "DatabaseNameChanged",
    "DatabaseDescriptionChanged",
    "DefaultUserNameChanged",
    "MasterKeyChanged",
    "RecycleBinChanged",
    "EntryTemplatesGroupChanged",
    "SettingsChanged",
];

/// An open database: the outer settings, the attachment pool and the XML
/// tree. Saving writes the tree back as it is; only what was edited changes.
pub struct Database {
    pub(crate) header: OuterHeader,
    /// inner stream cipher for the next save
    pub(crate) stream_id: u32,
    pub(crate) binaries: Vec<Binary>,
    pub(crate) root: Element,
    /// the version the file had when it was opened
    pub(crate) opened_version: (u16, u16),
    pub(crate) index: crate::view::Index,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // never print the contents
        write!(
            f,
            "Database(KDBX {}.{})",
            self.opened_version.0, self.opened_version.1
        )
    }
}

/// Outer settings that a save keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub cipher: Cipher,
    pub kdf: Kdf,
    pub compressed: bool,
}

impl Database {
    /// Decrypts a KDBX 3.1 or 4.x file.
    pub fn open(data: &[u8], key: &Key) -> Result<Database> {
        let header = OuterHeader::parse(data)?;
        let composite = key.composite()?;
        let db = if header.major >= 4 {
            open4(data, header, &composite)?
        } else {
            open3(data, header, &composite)?
        };
        Ok(db)
    }

    /// The decrypted inner XML exactly as stored (protected values still
    /// encrypted). For tools that compare files; the app never needs it.
    pub fn inner_xml(data: &[u8], key: &Key) -> Result<Zeroizing<String>> {
        let header = OuterHeader::parse(data)?;
        let composite = key.composite()?;
        let payload = if header.major >= 4 {
            let payload = decrypt4(data, &header, &composite)?;
            let inner = InnerHeader::parse(&payload)?;
            Zeroizing::new(payload[inner.len..].to_vec())
        } else {
            decrypt3(data, &header, &composite)?
        };
        let text =
            std::str::from_utf8(&payload).map_err(|_| Error::Corrupt("the XML is not UTF-8"))?;
        Ok(Zeroizing::new(text.to_string()))
    }

    pub fn opened_version(&self) -> (u16, u16) {
        self.opened_version
    }

    pub fn settings(&self) -> Settings {
        Settings {
            cipher: self.header.cipher,
            // validated on open
            kdf: Kdf::from_params(&self.header.kdf).unwrap_or(Kdf::Aes { rounds: 0 }),
            compressed: self.header.compression == COMPRESSION_GZIP,
        }
    }

    pub fn set_settings(&mut self, s: &Settings) {
        self.header.cipher = s.cipher;
        self.header.kdf = s.kdf.to_params(&self.header.kdf);
        self.header.compression = if s.compressed {
            COMPRESSION_GZIP
        } else {
            COMPRESSION_NONE
        };
    }

    /// The inner stream cipher id the next save uses.
    pub fn inner_stream(&self) -> u32 {
        self.stream_id
    }

    /// The document root (`KeePassFile`).
    pub fn document(&self) -> &Element {
        &self.root
    }

    pub fn binaries(&self) -> &[Binary] {
        &self.binaries
    }

    /// The document with protected values in plain text (see [`xml::write_plain`]).
    pub fn plain_xml(&self) -> Zeroizing<String> {
        xml::write_plain(&self.root)
    }

    /// Encrypts the database as KDBX 4.1 with fresh seeds, keeping the cipher,
    /// KDF parameters and compression. Runs the KDF once, then decrypts the
    /// result again with the same derived key and checks it holds the same
    /// tree and attachments.
    pub fn save(&self, key: &Key) -> Result<Vec<u8>> {
        let composite = key.composite()?;
        let mut header = self.header.clone();
        header.major = 4;
        header.minor = 1;
        header.master_seed = crypto::random::<32>().to_vec();
        header.iv = crypto::random_vec(header.cipher.iv_len());
        let seed_len = header.kdf.get("S").map_or(32, |s| s.len().clamp(16, 64));
        header.kdf.set(VD_BYTES, "S", crypto::random_vec(seed_len));
        let header_bytes = header.write_kdbx41();

        let transformed = transform_key(&composite, &header.kdf)?;
        let master_key = Zeroizing::new(sha256(&[&header.master_seed, &transformed[..]]));
        let hmac_base = sha512(&[&header.master_seed, &transformed[..], &[1]]);

        let inner = InnerHeader {
            stream_id: self.stream_id,
            stream_key: Zeroizing::new(crypto::random_vec(64)),
            binaries: self.binaries.clone(),
            len: 0,
        };
        let mut stream = InnerStream::new(inner.stream_id, &inner.stream_key)?;
        let body = xml::write(&self.root, &mut stream);
        let mut payload = Zeroizing::new(Vec::with_capacity(body.len() + 1024));
        inner.write(&mut payload);
        payload.extend_from_slice(&body);
        drop(body);
        let payload = if header.compression == COMPRESSION_GZIP {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            Zeroizing::new(
                gz.write_all(&payload)
                    .and_then(|_| gz.finish())
                    .map_err(|_| Error::Corrupt("compression failed"))?,
            )
        } else {
            payload
        };
        let encrypted = header.cipher.encrypt(&master_key, &header.iv, &payload);
        drop(payload);

        let mut out = Vec::with_capacity(encrypted.len() + header_bytes.len() + 256);
        out.extend_from_slice(&header_bytes);
        out.extend_from_slice(&sha256(&[&header_bytes]));
        let header_key = sha512(&[&u64::MAX.to_le_bytes(), &hmac_base[..]]);
        out.extend_from_slice(&crypto::hmac_sha256(&header_key[..], &[&header_bytes]));
        let chunks = encrypted.chunks(BLOCK_SIZE).chain(std::iter::once(&[][..]));
        for (i, block) in chunks.enumerate() {
            let index = (i as u64).to_le_bytes();
            let len = (block.len() as u32).to_le_bytes();
            let key = sha512(&[&index, &hmac_base[..]]);
            out.extend_from_slice(&crypto::hmac_sha256(&key[..], &[&index, &len, block]));
            out.extend_from_slice(&len);
            out.extend_from_slice(block);
        }

        // Read back what was just written before anyone replaces a file with
        // it: it must decrypt with this key and hold the same tree.
        let check = OuterHeader::parse(&out)
            .and_then(|h| {
                let payload = decrypt4_with(&out, &h, &composite, Some(&transformed))?;
                finish4(h, &payload)
            })
            .map_err(|_| Error::Corrupt("the saved file does not read back"))?;
        if check.root != self.root || check.binaries != self.binaries {
            return Err(Error::Corrupt("the saved file does not read back"));
        }
        Ok(out)
    }

    pub(crate) fn from_parts(
        header: OuterHeader,
        stream_id: u32,
        binaries: Vec<Binary>,
        root: Element,
    ) -> Result<Database> {
        if root.name != "KeePassFile" {
            return Err(Error::Corrupt("the XML is not a KeePass database"));
        }
        let opened_version = (header.major, header.minor);
        let mut db = Database {
            header,
            stream_id,
            binaries,
            root,
            opened_version,
            index: Default::default(),
        };
        db.reindex();
        Ok(db)
    }
}

pub(crate) fn gunzip(data: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::with_capacity(data.len() * 4));
    flate2::read::GzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|_| Error::Corrupt("cannot decompress"))?;
    Ok(out)
}

// ---------------------------------------------------------------- KDBX 4

/// Verifies the header and the HMAC blocks and decrypts. Returns the plaintext
/// payload (inner header + XML), decompressed.
fn decrypt4(data: &[u8], header: &OuterHeader, composite: &[u8; 32]) -> Result<Zeroizing<Vec<u8>>> {
    decrypt4_with(data, header, composite, None)
}

/// [`decrypt4`] with the KDF result already known (checking a file just
/// written), so the KDF does not run again.
fn decrypt4_with(
    data: &[u8],
    header: &OuterHeader,
    composite: &[u8; 32],
    known: Option<&[u8; 32]>,
) -> Result<Zeroizing<Vec<u8>>> {
    let header_bytes = &data[..header.len];
    let mut r = Reader::new(data);
    r.take(header.len, "truncated header")?;
    let hash = r.take(32, "truncated header")?;
    if sha256(&[header_bytes])[..] != *hash {
        return Err(Error::Corrupt("header checksum mismatch"));
    }
    let tag = r.take(32, "truncated header")?;
    let transformed = match known {
        Some(t) => Zeroizing::new(*t),
        None => transform_key(composite, &header.kdf)?,
    };
    let hmac_base = sha512(&[&header.master_seed, &transformed[..], &[1]]);
    let header_key = sha512(&[&u64::MAX.to_le_bytes(), &hmac_base[..]]);
    if !crypto::hmac_verify(&header_key[..], &[header_bytes], tag) {
        return Err(Error::WrongKey);
    }

    let mut buf = Zeroizing::new(Vec::with_capacity(r.rest().len()));
    for index in 0u64.. {
        let what = "truncated data";
        let tag = r.take(32, what)?;
        let len_bytes = r.take(4, what)?;
        let len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]);
        let block = r.take(len as usize, what)?;
        let key = sha512(&[&index.to_le_bytes(), &hmac_base[..]]);
        if !crypto::hmac_verify(&key[..], &[&index.to_le_bytes(), len_bytes, block], tag) {
            return Err(Error::Corrupt("block checksum mismatch"));
        }
        if len == 0 {
            break;
        }
        buf.extend_from_slice(block);
    }
    let master_key = Zeroizing::new(sha256(&[&header.master_seed, &transformed[..]]));
    header
        .cipher
        .decrypt_raw(&master_key, &header.iv, &mut buf)?;
    let n = header.cipher.unpadded_len(&buf)?;
    buf.truncate(n);
    let payload = if header.compression == COMPRESSION_GZIP {
        gunzip(&buf)?
    } else {
        buf
    };
    Ok(payload)
}

fn open4(data: &[u8], header: OuterHeader, composite: &[u8; 32]) -> Result<Database> {
    let payload = decrypt4(data, &header, composite)?;
    finish4(header, &payload)
}

/// Everything after decryption: inner header and XML.
pub(crate) fn finish4(header: OuterHeader, payload: &[u8]) -> Result<Database> {
    let inner = InnerHeader::parse(payload)?;
    let mut stream = InnerStream::new(inner.stream_id, &inner.stream_key)?;
    let text = std::str::from_utf8(&payload[inner.len..])
        .map_err(|_| Error::Corrupt("the XML is not UTF-8"))?;
    let root = xml::parse(text, &mut stream)?;
    Database::from_parts(header, inner.stream_id, inner.binaries, root)
}

// ---------------------------------------------------------------- KDBX 3.1

/// Decrypts, checks the start bytes (a wrong key fails here, before the
/// padding: M0 issue K5) and unpacks the hashed blocks.
fn decrypt3(data: &[u8], header: &OuterHeader, composite: &[u8; 32]) -> Result<Zeroizing<Vec<u8>>> {
    let encrypted = data.get(header.len..).unwrap_or(&[]);
    let start = header.stream_start.as_deref().unwrap_or(&[]);
    if encrypted.len() < start.len() {
        return Err(Error::Corrupt("truncated data"));
    }
    let transformed = transform_key(composite, &header.kdf)?;
    let master_key = Zeroizing::new(sha256(&[&header.master_seed, &transformed[..]]));

    // CBC decrypts each block on its own given the previous one, so the start
    // bytes can be checked even when the rest of the file is cut short
    let head_len = if header.cipher == Cipher::ChaCha20 {
        start.len().min(encrypted.len())
    } else {
        start.len().div_ceil(16).min(encrypted.len() / 16) * 16
    };
    let mut head = Zeroizing::new(encrypted[..head_len].to_vec());
    header
        .cipher
        .decrypt_raw(&master_key, &header.iv, &mut head)?;
    if head.len() < start.len() || head[..start.len()] != *start {
        return Err(Error::WrongKey);
    }

    let mut buf = Zeroizing::new(encrypted.to_vec());
    header
        .cipher
        .decrypt_raw(&master_key, &header.iv, &mut buf)?;
    let n = header.cipher.unpadded_len(&buf)?;
    if n < start.len() {
        return Err(Error::Corrupt("truncated data"));
    }

    let mut r = Reader::new(&buf[start.len()..n]);
    let mut out = Zeroizing::new(Vec::with_capacity(n));
    for index in 0u32.. {
        let what = "truncated data";
        if r.u32(what)? != index {
            return Err(Error::Corrupt("blocks out of order"));
        }
        let hash = r.take(32, what)?;
        let len = r.u32(what)? as usize;
        if len == 0 {
            if hash.iter().any(|&b| b != 0) {
                return Err(Error::Corrupt("bad final block"));
            }
            break;
        }
        let block = r.take(len, what)?;
        if sha256(&[block])[..] != *hash {
            return Err(Error::Corrupt("block checksum mismatch"));
        }
        out.extend_from_slice(block);
    }
    if header.compression == COMPRESSION_GZIP {
        gunzip(&out)
    } else {
        Ok(out)
    }
}

fn open3(data: &[u8], header: OuterHeader, composite: &[u8; 32]) -> Result<Database> {
    let payload = decrypt3(data, &header, composite)?;
    let header_hash = sha256(&[&data[..header.len]]);
    finish3(header, payload, header_hash)
}

/// Everything after decryption: the XML, then the conversion to the form
/// KDBX 4 stores.
pub(crate) fn finish3(
    header: OuterHeader,
    payload: Zeroizing<Vec<u8>>,
    header_hash: [u8; 32],
) -> Result<Database> {
    let stream_id = header.stream_id.unwrap_or(STREAM_SALSA20);
    let mut stream = InnerStream::new(
        stream_id,
        header
            .stream_key
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    )?;
    let text = std::str::from_utf8(&payload).map_err(|_| Error::Corrupt("the XML is not UTF-8"))?;
    let mut root = xml::parse(text, &mut stream)?;
    drop(payload);

    // ---- what KDBX 4 stores differently
    let mut binaries = Vec::new();
    if let Some(meta) = root.child_mut("Meta") {
        if let Some(h) = meta.remove_child("HeaderHash") {
            let stored = B64.decode(h.text().trim()).unwrap_or_default();
            if !stored.is_empty() && stored != header_hash {
                return Err(Error::Corrupt("header checksum mismatch"));
            }
        }
        if let Some(pool) = meta.remove_child("Binaries") {
            binaries = pool_from_meta(&pool)?;
        }
    }
    let ids: HashMap<String, usize> = binaries
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.clone(), i))
        .collect();
    if let Some(r) = root.child_mut("Root") {
        renumber_refs(r, &ids);
    }
    convert_times(&mut root);

    let mut header = header;
    header.stream_key = None;
    header.stream_start = None;
    header.stream_id = None;
    // KeePass switches to ChaCha20 when it upgrades a file to KDBX 4
    Database::from_parts(
        header,
        STREAM_CHACHA20,
        binaries.into_iter().map(|(_, b)| b).collect(),
        root,
    )
}

/// KDBX 3.1 keeps attachments in Meta/Binaries, Base64, maybe gzipped.
fn pool_from_meta(pool: &Element) -> Result<Vec<(String, Binary)>> {
    let mut out = Vec::new();
    for b in pool.children_named("Binary") {
        let id = b.attr("ID").unwrap_or("").trim().to_string();
        let protected = b.secret().is_some();
        let mut data = match b.secret() {
            Some(s) => Zeroizing::new(s.to_vec()),
            None => {
                let clean: String = b
                    .text()
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace())
                    .collect();
                Zeroizing::new(
                    B64.decode(clean)
                        .map_err(|_| Error::Corrupt("an attachment is not Base64"))?,
                )
            }
        };
        if b.attr("Compressed").is_some_and(is_true) && !data.is_empty() {
            data = gunzip(&data)?;
        }
        out.push((
            id,
            Binary {
                flags: u8::from(protected),
                data,
            },
        ));
    }
    Ok(out)
}

fn renumber_refs(el: &mut Element, ids: &HashMap<String, usize>) {
    if el.name == "Binary"
        && let Some(v) = el.child_mut("Value")
        && let Some(r) = v.attr("Ref")
        && let Some(i) = ids.get(r.trim())
    {
        let i = i.to_string();
        v.set_attr("Ref", &i);
    }
    for c in el.elements_mut() {
        renumber_refs(c, ids);
    }
}

fn convert_times(el: &mut Element) {
    if TIME_ELEMENTS.contains(&el.name.as_str())
        && el.elements().next().is_none()
        && let Some(t) = Timestamp::parse_iso(el.text().trim())
    {
        let keep_other: Vec<Node> = el
            .children
            .drain(..)
            .filter(|c| !matches!(c, Node::Text(_)))
            .collect();
        el.children = keep_other;
        el.children.insert(0, Node::Text(t.to_base64()));
        return;
    }
    for c in el.elements_mut() {
        convert_times(c);
    }
}
