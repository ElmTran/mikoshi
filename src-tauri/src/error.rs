//! Errors as the frontend sees them.
//!
//! Only a code goes over the wire (`{"code": "locked"}`); the UI turns it into
//! a translated message. Variants never carry user data, so an error can be
//! logged or shown without leaking anything.

use mikoshi_core::OpenError;
use serde::Serialize;

#[derive(Debug, Clone, Copy, thiserror::Error, Serialize)]
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
    #[error("a custom field name is empty, used twice or reserved")]
    FieldName,
    #[error("the expiry time cannot be used")]
    Expiry,
    #[error("the database has edits not saved yet")]
    UnsavedChanges,
    #[error("the file was changed by another program")]
    SaveConflict,
    #[error("the database cannot be written")]
    SaveFailed,
    #[error("saving turns the file into KDBX 4.1")]
    FormatUpgrade,
    #[error("not possible for this item")]
    NotAllowed,
    #[error("the name is empty")]
    EmptyName,
    #[error("a file with that name already exists")]
    FileExists,
    #[error("the file could not be written")]
    WriteFailed,
    #[error("the file is too large to attach")]
    TooLarge,
    #[error("no kind of character is selected")]
    NoCharacters,
    #[error("only http and https URLs are opened")]
    UnsafeUrl,
    #[error("the clipboard could not be written")]
    Clipboard,
    /// a feature that arrives in a later milestone
    #[error("not available yet")]
    NotYet,
    #[error("settings could not be saved")]
    SettingsNotSaved,
    #[error("the hotkey is not usable")]
    HotkeyInvalid,
    #[error("another program already uses the hotkey")]
    HotkeyTaken,
    #[error("start with Windows could not be changed")]
    Autostart,
    #[error("auto-type is turned off for this entry")]
    AutotypeDisabled,
    #[error("the auto-type sequence cannot be typed")]
    AutotypeSequence,
    #[error("the auto-type sequence uses a field the entry does not have")]
    AutotypeField,
    #[error("the window runs as administrator")]
    AutotypeElevated,
    #[error("there is no window to type into")]
    AutotypeNoWindow,
    #[error("auto-type was interrupted")]
    AutotypeFailed,
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

impl From<mikoshi_core::vault::CreateError> for AppError {
    fn from(e: mikoshi_core::vault::CreateError) -> Self {
        use mikoshi_core::vault::CreateError as C;
        match e {
            C::Exists => AppError::FileExists,
            C::NoKey => AppError::EmptyPassword,
            C::KeyFileUnreadable => AppError::KeyFileUnreadable,
            C::Kdf => AppError::Unsupported,
            C::Write => AppError::WriteFailed,
        }
    }
}

impl From<mikoshi_core::vault::SaveError> for AppError {
    fn from(e: mikoshi_core::vault::SaveError) -> Self {
        use mikoshi_core::vault::SaveError as S;
        match e {
            S::Conflict => AppError::SaveConflict,
            S::FormatUpgrade => AppError::FormatUpgrade,
            S::Encrypt | S::Write => AppError::SaveFailed,
        }
    }
}

impl From<mikoshi_core::vault::EditError> for AppError {
    fn from(e: mikoshi_core::vault::EditError) -> Self {
        use mikoshi_core::vault::EditError as E;
        match e {
            E::NotFound => AppError::NotFound,
            E::FieldName => AppError::FieldName,
            E::TotpInvalid => AppError::TotpInvalid,
            E::Expiry => AppError::Expiry,
            E::RecycleBin | E::NotAllowed => AppError::NotAllowed,
            E::EmptyName => AppError::EmptyName,
        }
    }
}
