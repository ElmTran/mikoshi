//! Errors as the frontend sees them.
//!
//! Only a code goes over the wire (`{"code": "locked"}`); the UI turns it into
//! a translated message. Variants never carry user data, so an error can be
//! logged or shown without leaking anything.

use mikoshi_core::OpenError;
use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum AppError {
    #[error("the database is locked")]
    Locked,
    #[error("no such entry, group or field")]
    NotFound,
    #[error("neither a master password nor a key file was given")]
    EmptyPassword,
    #[error("no database is selected")]
    NoDatabase,
    #[error("the database file does not exist")]
    FileNotFound,
    #[error("the database file cannot be read")]
    FileUnreadable,
    #[error("the key file cannot be read")]
    KeyFileUnreadable,
    #[error("wrong master password or key file")]
    WrongKey,
    #[error("wrong master password or key file, or a damaged file (KDBX 3.1)")]
    WrongKeyOrCorrupt,
    #[error("the database file is damaged")]
    Corrupt,
    #[error("unsupported database format or algorithm")]
    Unsupported,
    #[error("KeePass 1 databases are not supported")]
    KeePass1,
    #[error("not a KeePass database")]
    NotKeePass,
    #[error("the TOTP settings of this entry cannot be used")]
    TotpInvalid,
    #[error("only http and https URLs are opened")]
    UnsafeUrl,
    #[error("the clipboard could not be written")]
    Clipboard,
    /// a feature that arrives in a later milestone
    #[error("not available yet")]
    NotYet,
    #[error("settings could not be saved")]
    SettingsNotSaved,
    #[error("window operation failed")]
    Window,
    #[error("internal error")]
    Internal,
}

impl From<OpenError> for AppError {
    fn from(e: OpenError) -> Self {
        match e {
            OpenError::NoKey => AppError::EmptyPassword,
            OpenError::WrongKey => AppError::WrongKey,
            OpenError::WrongKeyOrCorrupt => AppError::WrongKeyOrCorrupt,
            OpenError::Corrupt => AppError::Corrupt,
            OpenError::Unsupported => AppError::Unsupported,
            OpenError::KeePass1 => AppError::KeePass1,
            OpenError::NotKeePass => AppError::NotKeePass,
            OpenError::FileNotFound => AppError::FileNotFound,
            OpenError::FileUnreadable => AppError::FileUnreadable,
            OpenError::KeyFileUnreadable => AppError::KeyFileUnreadable,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
