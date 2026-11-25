//! New databases (features.md §3.1, §3.8) and timing the KDF to calibrate
//! one.

use std::time::{Duration, Instant};

use crate::crypto::{self, STREAM_CHACHA20};
use crate::db::{Database, Settings};
use crate::error::Result;
use crate::header::{
    COMPRESSION_GZIP, COMPRESSION_NONE, Kdf, OuterHeader, VariantDict, transform_key,
};
use crate::types::{Timestamp, Uuid};
use crate::xml;

/// Argon2 version 1.3, the one every KeePass client writes.
pub const ARGON2_VERSION: u32 = 0x13;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

impl Database {
    /// An empty KDBX 4.1 database: a root group called `name`, the standard
    /// KeePass settings (password protected in memory, recycle bin on, 10
    /// history versions up to 6 MiB) and the given cipher, KDF and
    /// compression. The recycle bin is made on the first delete, as KeePass
    /// does. Nothing is written; [`Database::save`] produces the file.
    pub fn create(name: &str, settings: &Settings) -> Result<Database> {
        let now = Timestamp::now().to_base64();
        let nil = Uuid([0; 16]).to_base64();
        let root = Uuid::random().to_base64();
        let name = escape(name);
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\"?>
<KeePassFile>
\t<Meta>
\t\t<Generator>Mikoshi</Generator>
\t\t<DatabaseName>{name}</DatabaseName>
\t\t<DatabaseNameChanged>{now}</DatabaseNameChanged>
\t\t<DatabaseDescription/>
\t\t<DatabaseDescriptionChanged>{now}</DatabaseDescriptionChanged>
\t\t<DefaultUserName/>
\t\t<DefaultUserNameChanged>{now}</DefaultUserNameChanged>
\t\t<MaintenanceHistoryDays>365</MaintenanceHistoryDays>
\t\t<Color/>
\t\t<MasterKeyChanged>{now}</MasterKeyChanged>
\t\t<MasterKeyChangeRec>-1</MasterKeyChangeRec>
\t\t<MasterKeyChangeForce>-1</MasterKeyChangeForce>
\t\t<MemoryProtection>
\t\t\t<ProtectTitle>False</ProtectTitle>
\t\t\t<ProtectUserName>False</ProtectUserName>
\t\t\t<ProtectPassword>True</ProtectPassword>
\t\t\t<ProtectURL>False</ProtectURL>
\t\t\t<ProtectNotes>False</ProtectNotes>
\t\t</MemoryProtection>
\t\t<CustomIcons/>
\t\t<RecycleBinEnabled>True</RecycleBinEnabled>
\t\t<RecycleBinUUID>{nil}</RecycleBinUUID>
\t\t<RecycleBinChanged>{now}</RecycleBinChanged>
\t\t<EntryTemplatesGroup>{nil}</EntryTemplatesGroup>
\t\t<EntryTemplatesGroupChanged>{now}</EntryTemplatesGroupChanged>
\t\t<LastSelectedGroup>{nil}</LastSelectedGroup>
\t\t<LastTopVisibleGroup>{nil}</LastTopVisibleGroup>
\t\t<HistoryMaxItems>10</HistoryMaxItems>
\t\t<HistoryMaxSize>6291456</HistoryMaxSize>
\t\t<SettingsChanged>{now}</SettingsChanged>
\t\t<CustomData/>
\t</Meta>
\t<Root>
\t\t<Group>
\t\t\t<UUID>{root}</UUID>
\t\t\t<Name>{name}</Name>
\t\t\t<Notes/>
\t\t\t<IconID>48</IconID>
\t\t\t<Times>
\t\t\t\t<LastModificationTime>{now}</LastModificationTime>
\t\t\t\t<CreationTime>{now}</CreationTime>
\t\t\t\t<LastAccessTime>{now}</LastAccessTime>
\t\t\t\t<ExpiryTime>{now}</ExpiryTime>
\t\t\t\t<Expires>False</Expires>
\t\t\t\t<UsageCount>0</UsageCount>
\t\t\t\t<LocationChanged>{now}</LocationChanged>
\t\t\t</Times>
\t\t\t<IsExpanded>True</IsExpanded>
\t\t\t<DefaultAutoTypeSequence/>
\t\t\t<EnableAutoType>null</EnableAutoType>
\t\t\t<EnableSearching>null</EnableSearching>
\t\t\t<LastTopVisibleEntry>{nil}</LastTopVisibleEntry>
\t\t</Group>
\t\t<DeletedObjects/>
\t</Root>
</KeePassFile>
"
        );
        let stream_key = crypto::random_vec(64);
        let mut stream = crypto::InnerStream::new(STREAM_CHACHA20, &stream_key)?;
        let doc = xml::parse(&text, &mut stream)?;
        let header = OuterHeader {
            major: 4,
            minor: 1,
            cipher: settings.cipher,
            compression: if settings.compressed {
                COMPRESSION_GZIP
            } else {
                COMPRESSION_NONE
            },
            master_seed: crypto::random_vec(32),
            iv: crypto::random_vec(settings.cipher.iv_len()),
            kdf: settings.kdf.to_params(&VariantDict::new()),
            public_custom_data: None,
            stream_key: None,
            stream_start: None,
            stream_id: None,
            len: 0,
        };
        Database::from_parts(header, STREAM_CHACHA20, Vec::new(), doc)
    }
}

/// How long `kdf` takes on this machine, for calibrating a new database.
/// Runs it once on a throwaway key.
pub fn time_kdf(kdf: &Kdf) -> Result<Duration> {
    let mut params = kdf.to_params(&VariantDict::new());
    params.set(crate::header::VD_BYTES, "S", crypto::random_vec(32));
    let start = Instant::now();
    transform_key(&[0x5a; 32], &params)?;
    Ok(start.elapsed())
}

/// Argon2id with `memory` bytes and `parallelism` lanes, its iterations
/// chosen so one run takes about `target` here (at least 2, at most 10 000).
///
/// A run has a fixed cost (allocating the memory, starting the threads) and
/// a cost per iteration, so two short runs are timed and a line is fitted;
/// scaling a single short run would overestimate the per-iteration cost.
pub fn calibrate_argon2id(memory: u64, parallelism: u32, target: Duration) -> Result<Kdf> {
    calibrate(
        &Kdf::Argon2 {
            id: true,
            memory,
            iterations: 2,
            parallelism,
            version: ARGON2_VERSION,
        },
        target,
    )
}

/// `kdf` with its work factor (Argon2 iterations, AES-KDF rounds) chosen so
/// one run takes about `target` here; its other parameters stay. Argon2
/// iterations land between 2 and 10 000, AES-KDF rounds at 100 000 or more.
pub fn calibrate(kdf: &Kdf, target: Duration) -> Result<Kdf> {
    let with = |n: u64| match kdf.clone() {
        Kdf::Aes { .. } => Kdf::Aes { rounds: n },
        Kdf::Argon2 {
            id,
            memory,
            parallelism,
            version,
            ..
        } => Kdf::Argon2 {
            id,
            memory,
            iterations: n,
            parallelism,
            version,
        },
    };
    let (lo, hi, min, max) = match kdf {
        Kdf::Aes { .. } => (200_000u64, 2_000_000u64, 100_000u64, u64::MAX / 2),
        Kdf::Argon2 { .. } => (2, 10, 2, 10_000),
    };
    // the faster of two tries each: the first run also pays for warming up
    let time = |n| -> Result<f64> {
        let a = time_kdf(&with(n))?;
        let b = time_kdf(&with(n))?;
        Ok(a.min(b).as_secs_f64())
    };
    let (t_lo, t_hi) = (time(lo)?, time(hi)?);
    let per_unit = ((t_hi - t_lo) / (hi - lo) as f64).max(1e-12);
    let fixed = (t_lo - per_unit * lo as f64).max(0.0);
    let n = ((target.as_secs_f64() - fixed) / per_unit).round();
    Ok(with((n.max(0.0) as u64).clamp(min, max)))
}
