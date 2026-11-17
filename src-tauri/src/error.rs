//! Errors as the frontend sees them.
//!
//! Only a code goes over the wire (`{"code": "locked"}`); the UI turns it into
//! a translated message. Variants never carry user data, so an error can be
//! logged or shown without leaking anything.

use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum AppError {
    #[error("the database is locked")]
    Locked,
    #[error("no such entry, group or field")]
    NotFound,
    #[error("the master password is empty")]
    EmptyPassword,
    #[error("settings could not be saved")]
    SettingsNotSaved,
    #[error("window operation failed")]
    Window,
    #[error("internal error")]
    Internal,
}

pub type AppResult<T> = Result<T, AppError>;
