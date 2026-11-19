//! UUIDs and timestamps as KeePass stores them.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

/// A group, entry or icon id: 16 bytes, Base64 in the XML.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Uuid(pub [u8; 16]);

impl Uuid {
    pub const NIL: Uuid = Uuid([0; 16]);

    pub fn random() -> Uuid {
        Uuid(crate::crypto::random())
    }

    /// From the XML. `None` when malformed.
    pub fn from_base64(text: &str) -> Option<Uuid> {
        let bytes = B64.decode(text.trim()).ok()?;
        Some(Uuid(bytes.try_into().ok()?))
    }

    pub fn to_base64(self) -> String {
        B64.encode(self.0)
    }

    /// From the hyphenated form [`Display`](fmt::Display) gives; hyphens are optional.
    pub fn parse(text: &str) -> Option<Uuid> {
        let hex: Vec<u8> = text.bytes().filter(|b| *b != b'-').collect();
        if hex.len() != 32 {
            return None;
        }
        let mut out = [0u8; 16];
        for (i, pair) in hex.chunks(2).enumerate() {
            let s = std::str::from_utf8(pair).ok()?;
            out[i] = u8::from_str_radix(s, 16).ok()?;
        }
        Some(Uuid(out))
    }

    pub fn is_nil(self) -> bool {
        self == Uuid::NIL
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uuid({self})")
    }
}

/// A point in time, in whole seconds since 0001-01-01T00:00:00Z: the count
/// KDBX 4 stores (Base64 of a little-endian i64). KDBX 3.1 writes ISO 8601.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Timestamp(pub i64);

/// Seconds from 0001-01-01 to 1970-01-01.
const UNIX_OFFSET: i64 = 62_135_596_800;

/// Days since 1970-01-01 (proleptic Gregorian calendar).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl Timestamp {
    pub fn now() -> Timestamp {
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Timestamp(unix + UNIX_OFFSET)
    }

    pub fn from_unix(secs: i64) -> Timestamp {
        Timestamp(secs + UNIX_OFFSET)
    }

    pub fn unix(self) -> i64 {
        self.0 - UNIX_OFFSET
    }

    /// Either form; `None` when it is neither (the text is then kept as it is).
    pub fn parse(text: &str) -> Option<Timestamp> {
        let t = text.trim();
        Timestamp::parse_iso(t).or_else(|| Timestamp::parse_base64(t))
    }

    pub fn parse_base64(text: &str) -> Option<Timestamp> {
        let bytes = B64.decode(text.trim()).ok()?;
        let arr: [u8; 8] = bytes.try_into().ok()?;
        Some(Timestamp(i64::from_le_bytes(arr)))
    }

    /// `YYYY-MM-DDTHH:MM:SS`, optional fraction, then `Z` or `±HH:MM` (or nothing: UTC).
    pub fn parse_iso(text: &str) -> Option<Timestamp> {
        let b = text.as_bytes();
        if b.len() < 19
            || b[4] != b'-'
            || b[7] != b'-'
            || !matches!(b[10], b'T' | b't' | b' ')
            || b[13] != b':'
            || b[16] != b':'
        {
            return None;
        }
        let num = |r: std::ops::Range<usize>| -> Option<i64> {
            let s = text.get(r)?;
            if !s.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            s.parse().ok()
        };
        let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
        let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
        if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
            return None;
        }
        let mut rest = &text[19..];
        if let Some(r) = rest.strip_prefix('.') {
            let digits = r.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                return None;
            }
            rest = &r[digits..];
        }
        let offset = match rest {
            "" | "Z" | "z" => 0,
            _ => {
                let sign = match rest.as_bytes()[0] {
                    b'+' => 1,
                    b'-' => -1,
                    _ => return None,
                };
                let o = &rest[1..];
                if o.len() != 5 || o.as_bytes()[2] != b':' {
                    return None;
                }
                let oh: i64 = o.get(0..2)?.parse().ok()?;
                let om: i64 = o.get(3..5)?.parse().ok()?;
                sign * (oh * 3600 + om * 60)
            }
        };
        let days = days_from_civil(y, mo, d) - days_from_civil(1, 1, 1);
        Some(Timestamp(days * 86_400 + h * 3600 + mi * 60 + s - offset))
    }

    pub fn to_base64(self) -> String {
        B64.encode(self.0.to_le_bytes())
    }

    /// `YYYY-MM-DDTHH:MM:SSZ`
    pub fn to_iso(self) -> String {
        let days = self.0.div_euclid(86_400) + days_from_civil(1, 1, 1);
        let secs = self.0.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_forms() {
        let u = Uuid([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0, 1, 2, 3, 4, 5, 6, 7,
        ]);
        let s = u.to_string();
        assert_eq!(s, "01234567-89ab-cdef-0001-020304050607");
        assert_eq!(Uuid::parse(&s), Some(u));
        assert_eq!(Uuid::from_base64(&u.to_base64()), Some(u));
        assert_eq!(Uuid::from_base64("AAAA"), None);
    }

    #[test]
    fn iso_and_base64_agree() {
        let t = Timestamp::parse_iso("2024-01-02T03:04:05Z").unwrap();
        assert_eq!(t.to_iso(), "2024-01-02T03:04:05Z");
        assert_eq!(Timestamp::parse(&t.to_base64()), Some(t));
        assert_eq!(t.unix(), 1_704_164_645);
        assert_eq!(Timestamp(0).to_iso(), "0001-01-01T00:00:00Z");
        assert_eq!(
            Timestamp::parse_iso("2024-01-02T05:04:05.123+02:00"),
            Some(t)
        );
        assert_eq!(
            Timestamp::parse_iso("9999-12-31T23:59:59Z")
                .unwrap()
                .to_iso(),
            "9999-12-31T23:59:59Z"
        );
        assert_eq!(Timestamp::parse("AAAA"), None);
        assert_eq!(Timestamp::parse("2024-13-01T00:00:00Z"), None);
    }
}
