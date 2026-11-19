//! Errors. They carry no data from the file or the key, only fixed text, so
//! they can be logged and shown as they are.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// not a KeePass file at all
    NotKeePass,
    /// a KeePass 1 `.kdb` file
    KeePass1,
    /// a KeePass file with a version or algorithm this crate does not handle
    Unsupported(&'static str),
    /// the password or key file is wrong
    WrongKey,
    /// the file is damaged or truncated
    Corrupt(&'static str),
    /// the key file cannot be used
    KeyFile(&'static str),
    /// neither a password nor a key file
    NoKey,
    /// the KDF asks for more memory than can be had
    OutOfMemory,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotKeePass => f.write_str("not a KeePass file"),
            Error::KeePass1 => f.write_str("KeePass 1 (.kdb) files are not supported"),
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
            Error::WrongKey => f.write_str("wrong password or key file"),
            Error::Corrupt(what) => write!(f, "damaged file: {what}"),
            Error::KeyFile(what) => write!(f, "key file: {what}"),
            Error::NoKey => f.write_str("no password or key file given"),
            Error::OutOfMemory => f.write_str("not enough memory for the key derivation"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
