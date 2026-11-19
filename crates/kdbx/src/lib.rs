//! Lossless KDBX: reads KeePass 2 databases (KDBX 3.1, 4.0, 4.1) into an XML
//! tree that keeps every element, attribute and bit of whitespace, and writes
//! them back as KDBX 4.1 with only the edited nodes changed.
//!
//! Cryptography comes from RustCrypto (AES, Twofish, ChaCha20, Salsa20,
//! Argon2, SHA-2, HMAC) and flate2; nothing here implements an algorithm.
//! Malformed input gives an [`Error`], never a panic.
//!
//! ```no_run
//! # fn main() -> Result<(), kdbx::Error> {
//! let data = std::fs::read("db.kdbx").unwrap();
//! let key = kdbx::Key::new().with_password("secret");
//! let db = kdbx::Database::open(&data, &key)?;
//! for e in db.entries() {
//!     println!("{}", e.get("Title").unwrap_or_default());
//! }
//! let saved: Vec<u8> = db.save(&key)?;
//! # Ok(()) }
//! ```

mod crypto;
mod db;
mod error;
#[doc(hidden)]
pub mod fuzz;
mod header;
mod key;
mod types;
mod view;
pub mod xml;

pub use crypto::Cipher;
pub use db::{Database, Settings, TIME_ELEMENTS};
pub use error::{Error, Result};
pub use header::{Binary, Kdf};
pub use key::Key;
pub use types::{Timestamp, Uuid};
pub use view::{
    Association, Attachment, AutoType, CustomIcon, Entry, EntryMut, Field, Group, Meta, Times,
};

/// Inner stream cipher ids (KDBX `InnerRandomStreamID`).
pub mod stream {
    pub use crate::crypto::{
        STREAM_CHACHA20 as CHACHA20, STREAM_NONE as NONE, STREAM_SALSA20 as SALSA20,
    };
}

/// Reads only the signature and version: `(major, minor)`. Tells KeePass 1
/// files and non-KeePass files apart without a key.
pub fn version(data: &[u8]) -> Result<(u16, u16)> {
    header::read_version(data)
}
