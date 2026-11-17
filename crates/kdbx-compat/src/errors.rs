//! How an open failure would be explained to the user.

// one match per format reads better than a flattened one here
#![allow(clippy::collapsible_match)]

use keepass::error::{
    DatabaseFormatError, DatabaseKeyError, DatabaseOpenError, DatabaseVersionParseError as V,
    Kdbx3OpenError, Kdbx3OuterHeaderError as H3, Kdbx4OpenError, Kdbx4OuterHeaderError as H4,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// password or key file is wrong
    WrongKey,
    /// file is damaged or truncated
    Corrupt,
    /// a KeePass file, but a version or algorithm keepass-rs does not support
    Unsupported,
    /// not a KeePass file at all
    NotKeePass,
    /// the error does not tell a wrong key from a damaged file
    Ambiguous,
    Io,
}

impl Failure {
    pub fn label(self) -> &'static str {
        match self {
            Failure::WrongKey => "wrong key",
            Failure::Corrupt => "damaged",
            Failure::Unsupported => "unsupported",
            Failure::NotKeePass => "not a KeePass file",
            Failure::Ambiguous => "AMBIGUOUS (wrong key or damaged?)",
            Failure::Io => "I/O error",
        }
    }
}

pub fn classify(e: &DatabaseOpenError) -> Failure {
    match e {
        DatabaseOpenError::Key(DatabaseKeyError::IncorrectKey) => Failure::WrongKey,
        DatabaseOpenError::Key(_) => Failure::WrongKey,
        DatabaseOpenError::VersionParse(V::InvalidKDBXIdentifier)
        | DatabaseOpenError::VersionParse(V::UnexpectedEof) => Failure::NotKeePass,
        DatabaseOpenError::VersionParse(_) | DatabaseOpenError::UnsupportedVersion => {
            Failure::Unsupported
        }
        DatabaseOpenError::UnexpectedEof => Failure::Corrupt,
        // KDBX 3.1 has no header HMAC: with a wrong key, decryption usually
        // fails on the padding first, the same error a damaged file gives
        DatabaseOpenError::Cryptography(_) => Failure::Ambiguous,
        DatabaseOpenError::Format(DatabaseFormatError::Kdbx4(k)) => match k {
            Kdbx4OpenError::OuterHeader(H4::OuterCipherConfig(_))
            | Kdbx4OpenError::OuterHeader(H4::KdfConfig(_))
            | Kdbx4OpenError::OuterHeader(H4::CompressionConfig(_)) => Failure::Unsupported,
            _ => Failure::Corrupt,
        },
        DatabaseOpenError::Format(DatabaseFormatError::Kdbx3(k)) => match k {
            Kdbx3OpenError::OuterHeader(H3::OuterCipher(_))
            | Kdbx3OpenError::OuterHeader(H3::InnerCipher(_)) => Failure::Unsupported,
            _ => Failure::Corrupt,
        },
        DatabaseOpenError::Io(_) => Failure::Io,
        _ => Failure::Corrupt,
    }
}
