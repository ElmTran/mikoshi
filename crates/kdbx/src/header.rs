//! The unencrypted outer header (KDBX 3.1 and 4.x), the KDF parameters
//! ("VariantDictionary") and the KDBX 4 inner header.

use zeroize::Zeroizing;

use crate::crypto::{self, Cipher, KDF_AES_KDBX3, KDF_AES_KDBX4, KDF_ARGON2D, KDF_ARGON2ID, Key32};
use crate::error::{Error, Result};

pub const SIG1: u32 = 0x9AA2_D903;
pub const SIG2_KDBX: u32 = 0xB54B_FB67;
pub const SIG2_KDB: u32 = 0xB54B_FB65;
pub const SIG2_KDBX_PRE: u32 = 0xB54B_FB66;

/// A cursor that never panics: every read checks the length first.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn rest(&self) -> &'a [u8] {
        self.data.get(self.pos..).unwrap_or(&[])
    }
    pub fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::Corrupt(what))?;
        let s = self.data.get(self.pos..end).ok_or(Error::Corrupt(what))?;
        self.pos = end;
        Ok(s)
    }
    pub fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.take(1, what)?[0])
    }
    pub fn u16(&mut self, what: &'static str) -> Result<u16> {
        Ok(u16::from_le_bytes(arr(self.take(2, what)?)))
    }
    pub fn u32(&mut self, what: &'static str) -> Result<u32> {
        Ok(u32::from_le_bytes(arr(self.take(4, what)?)))
    }
}

fn arr<const N: usize>(s: &[u8]) -> [u8; N] {
    let mut a = [0u8; N];
    a.copy_from_slice(s);
    a
}

fn u32_of(v: &[u8]) -> Option<u32> {
    v.try_into().ok().map(u32::from_le_bytes)
}
fn u64_of(v: &[u8]) -> Option<u64> {
    v.try_into().ok().map(u64::from_le_bytes)
}

// ---------------------------------------------------------------- VariantDictionary

pub const VD_UINT32: u8 = 0x04;
pub const VD_UINT64: u8 = 0x05;
pub const VD_BYTES: u8 = 0x42;

/// KDBX 4's typed key/value list, kept as raw items in file order so that
/// unknown keys survive a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDict {
    pub version: u16,
    pub items: Vec<(u8, String, Vec<u8>)>,
}

impl VariantDict {
    pub fn new() -> Self {
        VariantDict {
            version: 0x0100,
            items: Vec::new(),
        }
    }

    pub fn parse(data: &[u8]) -> Result<VariantDict> {
        let what = "bad KDF parameters";
        let mut r = Reader::new(data);
        let version = r.u16(what)?;
        if version & 0xFF00 > 0x0100 {
            return Err(Error::Unsupported("KDF parameter format"));
        }
        let mut items = Vec::new();
        loop {
            let kind = r.u8(what)?;
            if kind == 0 {
                break;
            }
            let klen = r.u32(what)? as usize;
            let key = std::str::from_utf8(r.take(klen, what)?)
                .map_err(|_| Error::Corrupt(what))?
                .to_string();
            let vlen = r.u32(what)? as usize;
            let value = r.take(vlen, what)?.to_vec();
            items.push((kind, key, value));
        }
        Ok(VariantDict { version, items })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.version.to_le_bytes());
        for (kind, key, value) in &self.items {
            out.push(*kind);
            out.extend_from_slice(&(key.len() as u32).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(&(value.len() as u32).to_le_bytes());
            out.extend_from_slice(value);
        }
        out.push(0);
    }

    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.items
            .iter()
            .find(|(_, k, _)| k == key)
            .map(|(_, _, v)| v.as_slice())
    }

    pub fn get_u32(&self, key: &str) -> Option<u32> {
        self.get(key).and_then(u32_of)
    }
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(u64_of)
    }

    /// Replaces the value of an existing key in place, or appends it.
    pub fn set(&mut self, kind: u8, key: &str, value: Vec<u8>) {
        match self.items.iter_mut().find(|(_, k, _)| k == key) {
            Some(item) => {
                item.0 = kind;
                item.2 = value;
            }
            None => self.items.push((kind, key.to_string(), value)),
        }
    }
}

impl Default for VariantDict {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------- KDF

/// The key derivation function, read from the KDF parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kdf {
    Aes {
        rounds: u64,
    },
    Argon2 {
        id: bool,
        memory: u64,
        iterations: u64,
        parallelism: u32,
        version: u32,
    },
}

impl Kdf {
    pub fn from_params(p: &VariantDict) -> Result<Kdf> {
        let missing = Error::Corrupt("KDF parameter missing");
        let uuid = p.get("$UUID").ok_or(missing)?;
        if uuid == KDF_AES_KDBX3 || uuid == KDF_AES_KDBX4 {
            return Ok(Kdf::Aes {
                rounds: p.get_u64("R").ok_or(missing)?,
            });
        }
        let id = if uuid == KDF_ARGON2D {
            false
        } else if uuid == KDF_ARGON2ID {
            true
        } else {
            return Err(Error::Unsupported("key derivation function"));
        };
        Ok(Kdf::Argon2 {
            id,
            memory: p.get_u64("M").ok_or(missing)?,
            iterations: p.get_u64("I").ok_or(missing)?,
            parallelism: p.get_u32("P").ok_or(missing)?,
            version: p.get_u32("V").ok_or(missing)?,
        })
    }

    /// Parameters for this KDF, keeping the other items of `base` (unknown
    /// keys, the Argon2 secret) when it is the same kind of KDF.
    pub fn to_params(&self, base: &VariantDict) -> VariantDict {
        let same_kind = Kdf::from_params(base)
            .is_ok_and(|k| std::mem::discriminant(&k) == std::mem::discriminant(self));
        let mut p = if same_kind {
            base.clone()
        } else {
            VariantDict::new()
        };
        match self {
            Kdf::Aes { rounds } => {
                let uuid = match p.get("$UUID") {
                    Some(u) if u == KDF_AES_KDBX4 => KDF_AES_KDBX4,
                    _ => KDF_AES_KDBX3,
                };
                p.set(VD_BYTES, "$UUID", uuid.to_vec());
                p.set(VD_UINT64, "R", rounds.to_le_bytes().to_vec());
                if p.get("S").is_none() {
                    p.set(VD_BYTES, "S", vec![0; 32]);
                }
            }
            Kdf::Argon2 {
                id,
                memory,
                iterations,
                parallelism,
                version,
            } => {
                let uuid = if *id { KDF_ARGON2ID } else { KDF_ARGON2D };
                p.set(VD_BYTES, "$UUID", uuid.to_vec());
                if p.get("S").is_none() {
                    p.set(VD_BYTES, "S", vec![0; 32]);
                }
                p.set(VD_UINT32, "P", parallelism.to_le_bytes().to_vec());
                p.set(VD_UINT64, "M", memory.to_le_bytes().to_vec());
                p.set(VD_UINT64, "I", iterations.to_le_bytes().to_vec());
                p.set(VD_UINT32, "V", version.to_le_bytes().to_vec());
            }
        }
        p
    }
}

/// Runs the KDF described by `params` on the composite key.
pub fn transform_key(composite: &[u8; 32], params: &VariantDict) -> Result<Key32> {
    let seed = params
        .get("S")
        .ok_or(Error::Corrupt("KDF parameter missing"))?;
    match Kdf::from_params(params)? {
        Kdf::Aes { rounds } => crypto::aes_kdf(composite, seed, rounds),
        Kdf::Argon2 {
            id,
            memory,
            iterations,
            parallelism,
            version,
        } => crypto::argon2_kdf(
            composite,
            &crypto::Argon2Params {
                id,
                salt: seed,
                memory_bytes: memory,
                iterations,
                parallelism,
                version,
                secret: params.get("K"),
                assoc: params.get("A"),
            },
        ),
    }
}

// ---------------------------------------------------------------- outer header

pub const COMPRESSION_NONE: u32 = 0;
pub const COMPRESSION_GZIP: u32 = 1;

#[derive(Debug, Clone)]
pub struct OuterHeader {
    pub major: u16,
    pub minor: u16,
    pub cipher: Cipher,
    pub compression: u32,
    pub master_seed: Vec<u8>,
    pub iv: Vec<u8>,
    /// KDF parameters; built from TransformSeed / TransformRounds for 3.1
    pub kdf: VariantDict,
    /// field 12, kept byte for byte
    pub public_custom_data: Option<Vec<u8>>,
    // KDBX 3.1 only
    pub stream_key: Option<Zeroizing<Vec<u8>>>,
    pub stream_start: Option<Vec<u8>>,
    pub stream_id: Option<u32>,
    /// length of the header in the file, signature included
    pub len: usize,
}

/// Reads the signature and version. Tells KeePass 1 and non-KeePass files apart.
pub fn read_version(data: &[u8]) -> Result<(u16, u16)> {
    let mut r = Reader::new(data);
    let sig1 = r.u32("").map_err(|_| Error::NotKeePass)?;
    let sig2 = r.u32("").map_err(|_| Error::NotKeePass)?;
    if sig1 != SIG1 {
        return Err(Error::NotKeePass);
    }
    match sig2 {
        SIG2_KDBX => {}
        SIG2_KDB => return Err(Error::KeePass1),
        SIG2_KDBX_PRE => return Err(Error::Unsupported("pre-release KeePass 2 file")),
        _ => return Err(Error::NotKeePass),
    }
    let minor = r.u16("truncated header")?;
    let major = r.u16("truncated header")?;
    if !(major == 3 || major == 4) {
        return Err(Error::Unsupported("KDBX version"));
    }
    Ok((major, minor))
}

impl OuterHeader {
    pub fn parse(data: &[u8]) -> Result<OuterHeader> {
        let (major, minor) = read_version(data)?;
        let wide = major >= 4;
        let what = "truncated header";
        let mut r = Reader::new(data);
        r.take(12, what)?;

        let mut cipher = None;
        let mut compression = None;
        let mut master_seed = None;
        let mut iv = None;
        let mut kdf = None;
        let mut transform_seed = None;
        let mut transform_rounds = None;
        let mut public_custom_data = None;
        let mut stream_key = None;
        let mut stream_start = None;
        let mut stream_id = None;
        loop {
            let kind = r.u8(what)?;
            let len = if wide {
                r.u32(what)? as usize
            } else {
                usize::from(r.u16(what)?)
            };
            let body = r.take(len, what)?;
            match kind {
                0 => break,
                // 1 is a comment; KeePass ignores it too
                2 => cipher = Some(Cipher::from_uuid(body)?),
                3 => compression = u32_of(body),
                4 => master_seed = Some(body.to_vec()),
                5 if !wide => transform_seed = Some(body.to_vec()),
                6 if !wide => transform_rounds = u64_of(body),
                7 => iv = Some(body.to_vec()),
                8 if !wide => stream_key = Some(Zeroizing::new(body.to_vec())),
                9 if !wide => stream_start = Some(body.to_vec()),
                10 if !wide => stream_id = u32_of(body),
                11 if wide => kdf = Some(VariantDict::parse(body)?),
                12 if wide => public_custom_data = Some(body.to_vec()),
                _ => {}
            }
        }
        let missing = |f: &'static str| Error::Corrupt(f);
        let compression = compression.ok_or(missing("compression flag missing"))?;
        if compression > COMPRESSION_GZIP {
            return Err(Error::Unsupported("compression algorithm"));
        }
        let cipher = cipher.ok_or(missing("cipher missing"))?;
        let kdf = match kdf {
            Some(k) => k,
            None if !wide => {
                let mut p = VariantDict::new();
                p.set(VD_BYTES, "$UUID", KDF_AES_KDBX3.to_vec());
                p.set(
                    VD_UINT64,
                    "R",
                    transform_rounds
                        .ok_or(missing("transform rounds missing"))?
                        .to_le_bytes()
                        .to_vec(),
                );
                p.set(
                    VD_BYTES,
                    "S",
                    transform_seed.ok_or(missing("transform seed missing"))?,
                );
                p
            }
            None => return Err(missing("KDF parameters missing")),
        };
        // reject unknown KDFs before asking for a key
        Kdf::from_params(&kdf)?;
        let master_seed = master_seed.ok_or(missing("master seed missing"))?;
        if master_seed.len() != 32 {
            return Err(Error::Corrupt("master seed has a bad length"));
        }
        let iv = iv.ok_or(missing("IV missing"))?;
        if !wide {
            stream_key.as_ref().ok_or(missing("stream key missing"))?;
            stream_start
                .as_ref()
                .ok_or(missing("start bytes missing"))?;
            stream_id.ok_or(missing("inner stream id missing"))?;
        }
        Ok(OuterHeader {
            major,
            minor,
            cipher,
            compression,
            master_seed,
            iv,
            kdf,
            public_custom_data,
            stream_key,
            stream_start,
            stream_id,
            len: r.pos(),
        })
    }

    /// The KDBX 4.1 header for a save.
    pub fn write_kdbx41(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&SIG1.to_le_bytes());
        out.extend_from_slice(&SIG2_KDBX.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        let mut field = |kind: u8, body: &[u8]| {
            out.push(kind);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(body);
        };
        field(2, &self.cipher.uuid());
        field(3, &self.compression.to_le_bytes());
        field(4, &self.master_seed);
        field(7, &self.iv);
        let mut kdf = Vec::new();
        self.kdf.write(&mut kdf);
        field(11, &kdf);
        if let Some(p) = &self.public_custom_data {
            field(12, p);
        }
        field(0, b"\r\n\r\n");
        out
    }
}

// ---------------------------------------------------------------- inner header

/// An attachment in the binary pool.
#[derive(Clone, PartialEq, Eq)]
pub struct Binary {
    /// KDBX 4 flags; bit 0 means "protect in memory"
    pub flags: u8,
    pub data: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for Binary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Binary({} bytes, flags {})", self.data.len(), self.flags)
    }
}

impl Binary {
    pub fn protected(&self) -> bool {
        self.flags & 1 != 0
    }
}

pub struct InnerHeader {
    pub stream_id: u32,
    pub stream_key: Zeroizing<Vec<u8>>,
    pub binaries: Vec<Binary>,
    /// where the XML starts
    pub len: usize,
}

impl InnerHeader {
    pub fn parse(data: &[u8]) -> Result<InnerHeader> {
        let what = "bad inner header";
        let mut r = Reader::new(data);
        let mut stream_id = None;
        let mut stream_key = None;
        let mut binaries = Vec::new();
        loop {
            let kind = r.u8(what)?;
            let len = r.u32(what)? as usize;
            let body = r.take(len, what)?;
            match kind {
                0 => break,
                1 => stream_id = u32_of(body),
                2 => stream_key = Some(Zeroizing::new(body.to_vec())),
                3 => {
                    let (&flags, data) = body.split_first().ok_or(Error::Corrupt(what))?;
                    binaries.push(Binary {
                        flags,
                        data: Zeroizing::new(data.to_vec()),
                    });
                }
                _ => {}
            }
        }
        Ok(InnerHeader {
            stream_id: stream_id.ok_or(Error::Corrupt("inner stream id missing"))?,
            stream_key: stream_key.ok_or(Error::Corrupt("inner stream key missing"))?,
            binaries,
            len: r.pos(),
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        let mut field = |kind: u8, parts: &[&[u8]]| {
            let len: usize = parts.iter().map(|p| p.len()).sum();
            out.push(kind);
            out.extend_from_slice(&(len as u32).to_le_bytes());
            for p in parts {
                out.extend_from_slice(p);
            }
        };
        field(1, &[&self.stream_id.to_le_bytes()]);
        field(2, &[&self.stream_key]);
        for b in &self.binaries {
            field(3, &[&[b.flags], &b.data]);
        }
        field(0, &[]);
    }
}
