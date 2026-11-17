//! Reads the unencrypted outer header of a KDBX file, so that the settings
//! before and after a save can be compared (and reported) without relying on
//! keepass-rs itself.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub version: String,
    pub cipher: String,
    pub compression: String,
    pub kdf: Kdf,
    /// KDBX 3.1 keeps the inner stream id in the outer header; KDBX 4 moved it
    /// into the encrypted inner header.
    pub inner_stream: Option<String>,
    pub has_public_custom_data: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kdf {
    /// AES-KDF; `kdbx4_uuid` tells which of the two UUIDs was used
    Aes {
        rounds: u64,
        uuid: &'static str,
    },
    Argon2 {
        variant: &'static str,
        memory: u64,
        iterations: u64,
        parallelism: u32,
        version: u32,
    },
    Unknown(String),
}

impl fmt::Display for Kdf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kdf::Aes { rounds, uuid } => write!(f, "AES-KDF ({uuid} UUID), {rounds} rounds"),
            Kdf::Argon2 {
                variant,
                memory,
                iterations,
                parallelism,
                version,
            } => write!(
                f,
                "{variant} v{version:#x}, {} MiB, {iterations} iterations, parallelism {parallelism}",
                memory / 1024 / 1024
            ),
            Kdf::Unknown(s) => write!(f, "unknown KDF {s}"),
        }
    }
}

impl fmt::Display for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "KDBX {}, {}, {}, {}",
            self.version, self.cipher, self.kdf, self.compression
        )?;
        if let Some(s) = &self.inner_stream {
            write!(f, ", inner stream {s}")?;
        }
        Ok(())
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn cipher_name(id: &[u8]) -> String {
    match hex(id).as_str() {
        "31c1f2e6bf714350be5805216afc5aff" => "AES-256".into(),
        "ad68f29f576f4bb9a36ad47af965346c" => "Twofish".into(),
        "d6038a2b8b6f4cb5a524339a31dbb59a" => "ChaCha20".into(),
        other => format!("unknown cipher {other}"),
    }
}

fn compression_name(v: &[u8]) -> String {
    match v.first() {
        Some(0) => "no compression".into(),
        Some(1) => "GZip".into(),
        _ => format!("unknown compression {}", hex(v)),
    }
}

fn stream_name(v: u32) -> String {
    match v {
        0 => "none".into(),
        1 => "ArcFour".into(),
        2 => "Salsa20".into(),
        3 => "ChaCha20".into(),
        other => format!("unknown {other}"),
    }
}

fn u16le(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
}
fn u32le(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}
fn u64le(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

pub fn read(data: &[u8]) -> Result<Header, String> {
    if data.get(0..4) != Some(&[0x03, 0xd9, 0xa2, 0x9a]) {
        return Err("not a KeePass file".into());
    }
    let minor = u16le(data, 8).ok_or("truncated")?;
    let major = u16le(data, 10).ok_or("truncated")?;
    let version = format!("{major}.{minor}");
    let wide = major >= 4;

    let mut pos = 12;
    let mut cipher = String::new();
    let mut compression = String::new();
    let mut kdf = Kdf::Unknown("missing".into());
    let mut inner_stream = None;
    let mut public = false;
    loop {
        let kind = *data.get(pos).ok_or("truncated header")?;
        let (len, skip) = if wide {
            (u32le(data, pos + 1).ok_or("truncated header")? as usize, 5)
        } else {
            (
                usize::from(u16le(data, pos + 1).ok_or("truncated header")?),
                3,
            )
        };
        let body = data
            .get(pos + skip..pos + skip + len)
            .ok_or("truncated header")?;
        pos += skip + len;
        match kind {
            0 => break,
            2 => cipher = cipher_name(body),
            3 => compression = compression_name(body),
            6 if !wide => {
                kdf = Kdf::Aes {
                    rounds: u64le(body, 0).unwrap_or(0),
                    uuid: "KDBX 3.1",
                }
            }
            10 if !wide => inner_stream = Some(stream_name(u32le(body, 0).unwrap_or(99))),
            11 => kdf = parse_kdf(body),
            12 => public = true,
            _ => {}
        }
    }
    Ok(Header {
        version,
        cipher,
        compression,
        kdf,
        inner_stream,
        has_public_custom_data: public,
    })
}

/// Parses the KDF parameters of a KDBX 4 header (a "VariantDictionary").
fn parse_kdf(d: &[u8]) -> Kdf {
    let mut fields = std::collections::HashMap::new();
    let mut pos = 2; // version
    while let Some(&kind) = d.get(pos) {
        if kind == 0 {
            break;
        }
        let Some(klen) = u32le(d, pos + 1) else { break };
        let kstart = pos + 5;
        let Some(key) = d.get(kstart..kstart + klen as usize) else {
            break;
        };
        let vlen_at = kstart + klen as usize;
        let Some(vlen) = u32le(d, vlen_at) else { break };
        let Some(value) = d.get(vlen_at + 4..vlen_at + 4 + vlen as usize) else {
            break;
        };
        fields.insert(String::from_utf8_lossy(key).to_string(), value.to_vec());
        pos = vlen_at + 4 + vlen as usize;
    }
    let get64 = |k: &str| fields.get(k).and_then(|v| u64le(v, 0)).unwrap_or(0);
    let get32 = |k: &str| fields.get(k).and_then(|v| u32le(v, 0)).unwrap_or(0);
    let uuid = fields.get("$UUID").map(|v| hex(v)).unwrap_or_default();
    match uuid.as_str() {
        "c9d9f39a628a4460bf740d08c18a4fea" => Kdf::Aes {
            rounds: get64("R"),
            uuid: "KDBX 3.1",
        },
        "7c02bb8279a74ac0927d114a00648238" => Kdf::Aes {
            rounds: get64("R"),
            uuid: "KDBX 4",
        },
        "ef636ddf8c29444b91f7a9a403e30a0c" => Kdf::Argon2 {
            variant: "Argon2d",
            memory: get64("M"),
            iterations: get64("I"),
            parallelism: get32("P"),
            version: get32("V"),
        },
        "9e298b1956db4773b23dfc3ec6f0a1e6" => Kdf::Argon2 {
            variant: "Argon2id",
            memory: get64("M"),
            iterations: get64("I"),
            parallelism: get32("P"),
            version: get32("V"),
        },
        other => Kdf::Unknown(other.to_string()),
    }
}
