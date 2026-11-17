//! A string that must never be logged, printed or kept around longer than
//! needed: master passwords, entry passwords, protected fields, TOTP secrets.
//!
//! - `Debug` and `Display` print `[redacted]`, so a stray `{:?}` in a log
//!   line or an error message cannot leak it.
//! - The memory is overwritten when the value is dropped.
//! - It deserializes from a plain string (so commands can take it as an
//!   argument) but deliberately does not implement `Serialize`: sending a
//!   secret to the frontend has to go through [`Secret::expose`], which makes
//!   every such place easy to find.

use std::fmt;

use serde::{Deserialize, Deserializer};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Default, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// The plain value. Every call site is a place where a secret leaves the
    /// type; keep them few and obvious.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_formats_its_value() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s}"), "[redacted]");
        assert_eq!(format!("{s:?}"), "[redacted]");
        assert_eq!(format!("{:?}", Some(&s)), "Some([redacted])");
        assert_eq!(s.expose(), "hunter2");
    }

    #[test]
    fn deserializes_from_a_string() {
        let s: Secret = serde_json::from_str("\"hunter2\"").unwrap();
        assert_eq!(s.expose(), "hunter2");
    }
}
