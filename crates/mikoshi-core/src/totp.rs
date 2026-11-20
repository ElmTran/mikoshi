//! TOTP codes (RFC 6238), computed on the Rust side so the secret never
//! reaches the frontend (features.md §6).
//!
//! Reads the three ways KeePass clients store TOTP settings, in the order
//! KeePassXC checks them:
//!
//! 1. the `otp` field: an `otpauth://totp/...` URI (KeePassXC, KeePassDX), or
//!    the KeeOtp form `key=...&step=30&size=6`
//! 2. KeePass 2's own fields: `TimeOtp-Secret[-Hex|-Base32|-Base64]`,
//!    `TimeOtp-Length`, `TimeOtp-Period`, `TimeOtp-Algorithm`
//! 3. the old KeePassXC / TrayTOTP fields `TOTP Seed` and `TOTP Settings`
//!    (`30;6`, or `30;S` for Steam)
//!
//! keepass-rs has its own TOTP, but it defaults to 8 digits when a URI leaves
//! them out and rejects lower-case secrets (M0 issues K3 and K4).

use hmac::{Hmac, Mac};
use zeroize::Zeroizing;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotpError {
    /// the entry has no TOTP settings
    Missing,
    /// TOTP settings are there but cannot be used (bad secret, HOTP, ...)
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    /// decimal digits
    Digits(u32),
    /// Steam Guard: 5 characters from a 26-letter alphabet
    Steam,
}

/// A code and how long it stays valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub code: String,
    pub period: u32,
    /// seconds until the next code
    pub remaining: u32,
}

/// TOTP settings with the decoded secret. The secret is wiped on drop and
/// never printed.
pub struct Totp {
    secret: Zeroizing<Vec<u8>>,
    period: u64,
    algorithm: Algorithm,
    encoder: Encoder,
}

impl std::fmt::Debug for Totp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Totp")
            .field("period", &self.period)
            .field("algorithm", &self.algorithm)
            .field("encoder", &self.encoder)
            .finish_non_exhaustive()
    }
}

const STEAM_ALPHABET: &[u8; 26] = b"23456789BCDFGHJKMNPQRTVWXY";
const DEFAULT_PERIOD: u64 = 30;
const DEFAULT_DIGITS: u32 = 6;

/// The fields that hold TOTP settings or secrets. They are never listed as
/// custom fields or revealed.
pub fn is_totp_field(key: &str) -> bool {
    key == "otp"
        || key == "TOTP Seed"
        || key == "TOTP Settings"
        || key.starts_with("TimeOtp-")
        || key.starts_with("HmacOtp-")
}

/// Whether the entry has TOTP settings at all (valid or not).
pub fn has_totp<'a>(get: impl Fn(&str) -> Option<&'a str>) -> bool {
    [
        "otp",
        "TOTP Seed",
        "TimeOtp-Secret",
        "TimeOtp-Secret-Hex",
        "TimeOtp-Secret-Base32",
        "TimeOtp-Secret-Base64",
    ]
    .iter()
    .any(|k| get(k).is_some_and(|v| !v.trim().is_empty()))
}

/// RFC 4648 Base32, forgiving like other KeePass clients: any case, spaces
/// and hyphens ignored, padding optional.
pub fn decode_base32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in text.chars() {
        let v = match c.to_ascii_uppercase() {
            c @ 'A'..='Z' => c as u32 - 'A' as u32,
            c @ '2'..='7' => c as u32 - '2' as u32 + 26,
            ' ' | '-' | '=' => continue,
            _ => return None,
        };
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if digits.is_empty() || !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
        .collect()
}

fn percent_decode(text: &str) -> String {
    percent_encoding::percent_decode_str(&text.replace('+', " "))
        .decode_utf8_lossy()
        .into_owned()
}

fn algorithm(name: &str) -> Option<Algorithm> {
    let n: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    match n.trim_start_matches("HMAC") {
        "SHA1" => Some(Algorithm::Sha1),
        "SHA256" => Some(Algorithm::Sha256),
        "SHA512" => Some(Algorithm::Sha512),
        _ => None,
    }
}

fn digits(text: &str) -> Option<u32> {
    text.trim().parse().ok().filter(|d| (4..=10).contains(d))
}

fn period(text: &str) -> Option<u64> {
    text.trim().parse().ok().filter(|p| *p > 0)
}

impl Totp {
    pub fn period(&self) -> u64 {
        self.period
    }

    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    pub fn encoder(&self) -> Encoder {
        self.encoder
    }

    /// The `otp` field value KeePassXC writes for a secret typed in by hand:
    /// `otpauth://totp/ISSUER:USER?secret=...&period=..&digits=..&issuer=ISSUER`,
    /// plus `algorithm` when it is not SHA-1. The secret is Base32 in any
    /// case, spaces, hyphens and padding allowed. Checked by parsing it back.
    pub fn manual_uri(
        secret: &str,
        algorithm: Algorithm,
        digits: u32,
        period: u64,
        issuer: &str,
        user: &str,
    ) -> Result<Zeroizing<String>, TotpError> {
        use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
        let clean: Zeroizing<String> = Zeroizing::new(
            secret
                .chars()
                .filter(|c| !matches!(c, ' ' | '-' | '=') && !c.is_whitespace())
                .collect::<String>()
                .to_ascii_uppercase(),
        );
        if decode_base32(&clean).is_none_or(|s| s.is_empty())
            || !(4..=10).contains(&digits)
            || period == 0
        {
            return Err(TotpError::Invalid);
        }
        let enc = |s: &str| utf8_percent_encode(s, NON_ALPHANUMERIC).to_string();
        let issuer = if issuer.trim().is_empty() {
            "Mikoshi"
        } else {
            issuer.trim()
        };
        let mut uri = Zeroizing::new(format!(
            "otpauth://totp/{}:{}?secret={}&period={period}&digits={digits}&issuer={}",
            enc(issuer),
            enc(user.trim()),
            clean.as_str(),
            enc(issuer),
        ));
        match algorithm {
            Algorithm::Sha1 => {}
            Algorithm::Sha256 => uri.push_str("&algorithm=SHA256"),
            Algorithm::Sha512 => uri.push_str("&algorithm=SHA512"),
        }
        Totp::parse_uri(&uri)?;
        Ok(uri)
    }

    fn new(
        secret: Vec<u8>,
        period: u64,
        algorithm: Algorithm,
        encoder: Encoder,
    ) -> Result<Totp, TotpError> {
        let secret = Zeroizing::new(secret);
        if secret.is_empty() {
            return Err(TotpError::Invalid);
        }
        Ok(Totp {
            secret,
            period,
            algorithm,
            encoder,
        })
    }

    /// Reads TOTP settings from an entry's fields (see the module docs).
    pub fn from_fields<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Result<Totp, TotpError> {
        let field = |k: &str| get(k).map(str::trim).filter(|v| !v.is_empty());

        if let Some(otp) = field("otp") {
            return Totp::parse_otp(otp);
        }

        let secret = if let Some(s) = field("TimeOtp-Secret-Base32") {
            Some(decode_base32(s))
        } else if let Some(s) = field("TimeOtp-Secret-Hex") {
            Some(decode_hex(s))
        } else if let Some(s) = field("TimeOtp-Secret-Base64") {
            Some(B64.decode(s.as_bytes()).ok())
        } else {
            // the plain secret may legitimately start or end with spaces
            get("TimeOtp-Secret")
                .filter(|s| !s.is_empty())
                .map(|s| Some(s.as_bytes().to_vec()))
        };
        if let Some(secret) = secret {
            let secret = secret.ok_or(TotpError::Invalid)?;
            let len = match field("TimeOtp-Length") {
                Some(l) => digits(l).ok_or(TotpError::Invalid)?,
                None => DEFAULT_DIGITS,
            };
            let per = match field("TimeOtp-Period") {
                Some(p) => period(p).ok_or(TotpError::Invalid)?,
                None => DEFAULT_PERIOD,
            };
            let alg = match field("TimeOtp-Algorithm") {
                Some(a) => algorithm(a).ok_or(TotpError::Invalid)?,
                None => Algorithm::Sha1,
            };
            return Totp::new(secret, per, alg, Encoder::Digits(len));
        }

        if let Some(seed) = field("TOTP Seed") {
            if seed.starts_with("otpauth://") {
                return Totp::parse_uri(seed);
            }
            let secret = decode_base32(seed).ok_or(TotpError::Invalid)?;
            let (mut per, mut enc) = (DEFAULT_PERIOD, Encoder::Digits(DEFAULT_DIGITS));
            if let Some(settings) = field("TOTP Settings") {
                let mut parts = settings.split(';');
                if let Some(p) = parts.next() {
                    per = period(p).ok_or(TotpError::Invalid)?;
                }
                if let Some(d) = parts.next() {
                    enc = if d.trim().eq_ignore_ascii_case("S") {
                        Encoder::Steam
                    } else {
                        Encoder::Digits(digits(d).ok_or(TotpError::Invalid)?)
                    };
                }
            }
            return Totp::new(secret, per, Algorithm::Sha1, enc);
        }

        Err(TotpError::Missing)
    }

    /// The `otp` field: an otpauth URI or the KeeOtp `key=...` form.
    pub fn parse_otp(text: &str) -> Result<Totp, TotpError> {
        if text.starts_with("otpauth://") {
            Totp::parse_uri(text)
        } else if text.contains("key=") {
            Totp::parse_params(text, false)
        } else {
            Err(TotpError::Invalid)
        }
    }

    /// `otpauth://totp/LABEL?secret=...&digits=6&period=30&algorithm=SHA1`
    /// (Google Authenticator's key URI format). Missing values take the
    /// standard defaults: 6 digits, 30 seconds, SHA-1.
    pub fn parse_uri(uri: &str) -> Result<Totp, TotpError> {
        let rest = uri.strip_prefix("otpauth://").ok_or(TotpError::Invalid)?;
        let (kind, rest) = rest.split_once('/').ok_or(TotpError::Invalid)?;
        if !kind.eq_ignore_ascii_case("totp") {
            // HOTP needs a counter that would have to be written back
            return Err(TotpError::Invalid);
        }
        let (label, query) = rest.split_once('?').unwrap_or((rest, ""));
        let steam_label = percent_decode(label)
            .to_ascii_lowercase()
            .starts_with("steam:");
        let mut totp = Totp::parse_params(query, true)?;
        if steam_label && matches!(totp.encoder, Encoder::Digits(DEFAULT_DIGITS)) {
            // KeePassXC marks Steam entries by issuer as well as by encoder
            let issuer_steam = query
                .split('&')
                .filter_map(|p| p.split_once('='))
                .any(|(k, v)| k.eq_ignore_ascii_case("issuer") && percent_decode(v) == "Steam");
            if issuer_steam {
                totp.encoder = Encoder::Steam;
            }
        }
        Ok(totp)
    }

    /// `secret`/`digits`/`period`/`algorithm` (URI) or
    /// `key`/`size`/`step`/`otpHashMode` (KeeOtp).
    fn parse_params(query: &str, uri: bool) -> Result<Totp, TotpError> {
        let (mut secret, mut per, mut alg) = (None, DEFAULT_PERIOD, Algorithm::Sha1);
        let mut enc = Encoder::Digits(DEFAULT_DIGITS);
        let mut steam = false;
        for pair in query.split('&') {
            let Some((k, v)) = pair.split_once('=') else {
                continue;
            };
            let v = percent_decode(v);
            match k.to_ascii_lowercase().as_str() {
                "secret" if uri => secret = Some(decode_base32(&v).ok_or(TotpError::Invalid)?),
                "key" if !uri => secret = Some(decode_base32(&v).ok_or(TotpError::Invalid)?),
                "digits" | "size" => {
                    enc = Encoder::Digits(digits(&v).ok_or(TotpError::Invalid)?);
                }
                "period" | "step" => per = period(&v).ok_or(TotpError::Invalid)?,
                "algorithm" | "otphashmode" => alg = algorithm(&v).ok_or(TotpError::Invalid)?,
                "encoder" => steam = v.eq_ignore_ascii_case("steam"),
                "type" if !uri && !v.eq_ignore_ascii_case("totp") => {
                    return Err(TotpError::Invalid);
                }
                _ => {}
            }
        }
        if steam {
            enc = Encoder::Steam;
        }
        Totp::new(secret.ok_or(TotpError::Invalid)?, per, alg, enc)
    }

    fn hmac(&self, message: &[u8]) -> Vec<u8> {
        fn run<M: Mac + hmac::digest::KeyInit>(key: &[u8], message: &[u8]) -> Vec<u8> {
            // HMAC takes keys of any length; this cannot fail
            let mut mac = <M as hmac::digest::KeyInit>::new_from_slice(key)
                .expect("HMAC accepts any key length");
            mac.update(message);
            mac.finalize().into_bytes().to_vec()
        }
        match self.algorithm {
            Algorithm::Sha1 => run::<Hmac<sha1::Sha1>>(&self.secret, message),
            Algorithm::Sha256 => run::<Hmac<sha2::Sha256>>(&self.secret, message),
            Algorithm::Sha512 => run::<Hmac<sha2::Sha512>>(&self.secret, message),
        }
    }

    /// The code for a Unix time in seconds.
    pub fn code_at(&self, unix: u64) -> Code {
        let counter = unix / self.period;
        let mac = Zeroizing::new(self.hmac(&counter.to_be_bytes()));
        let offset = (mac[mac.len() - 1] & 0x0f) as usize;
        let mut value = u32::from_be_bytes([
            mac[offset],
            mac[offset + 1],
            mac[offset + 2],
            mac[offset + 3],
        ]) & 0x7fff_ffff;
        let code = match self.encoder {
            Encoder::Digits(d) => {
                let modulus = 10u64.pow(d);
                format!("{:0width$}", u64::from(value) % modulus, width = d as usize)
            }
            Encoder::Steam => (0..5)
                .map(|_| {
                    let c = STEAM_ALPHABET[(value % 26) as usize] as char;
                    value /= 26;
                    c
                })
                .collect(),
        };
        Code {
            code,
            period: self.period as u32,
            remaining: (self.period - unix % self.period) as u32,
        }
    }

    /// The current code.
    pub fn now(&self) -> Code {
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.code_at(unix)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    // RFC 6238 appendix B: the secrets are ASCII digits of these lengths
    const SHA1_SECRET: &str = "12345678901234567890";
    const SHA256_SECRET: &str = "12345678901234567890123456789012";
    const SHA512_SECRET: &str = "1234567890123456789012345678901234567890123456789012345678901234";

    fn base32(bytes: &[u8]) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut out = String::new();
        let (mut buffer, mut bits) = (0u32, 0);
        for &b in bytes {
            buffer = (buffer << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out.push(A[((buffer >> bits) & 31) as usize] as char);
            }
        }
        if bits > 0 {
            out.push(A[((buffer << (5 - bits)) & 31) as usize] as char);
        }
        out
    }

    fn fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn from(map: &HashMap<String, String>) -> Result<Totp, TotpError> {
        Totp::from_fields(|k| map.get(k).map(String::as_str))
    }

    #[test]
    fn matches_rfc_6238_vectors() {
        let cases = [
            (
                "SHA1",
                SHA1_SECRET,
                [
                    (59, "94287082"),
                    (1111111109, "07081804"),
                    (20000000000, "65353130"),
                ],
            ),
            (
                "SHA256",
                SHA256_SECRET,
                [
                    (59, "46119246"),
                    (1111111109, "68084774"),
                    (20000000000, "77737706"),
                ],
            ),
            (
                "SHA512",
                SHA512_SECRET,
                [
                    (59, "90693936"),
                    (1111111109, "25091201"),
                    (20000000000, "47863826"),
                ],
            ),
        ];
        for (alg, secret, vectors) in cases {
            let uri = format!(
                "otpauth://totp/rfc?secret={}&digits=8&algorithm={alg}",
                base32(secret.as_bytes())
            );
            let totp = Totp::parse_uri(&uri).unwrap();
            for (t, code) in vectors {
                assert_eq!(totp.code_at(t).code, code, "{alg} at {t}");
            }
        }
    }

    #[test]
    fn defaults_to_six_digits_and_thirty_seconds() {
        // M0 issue K3: keepass-rs gives 8 digits here
        let totp = Totp::parse_uri(&format!(
            "otpauth://totp/minimal?secret={}",
            base32(SHA1_SECRET.as_bytes())
        ))
        .unwrap();
        let c = totp.code_at(59);
        assert_eq!(c.code, "287082");
        assert_eq!(c.period, 30);
        assert_eq!(c.remaining, 1);
    }

    #[test]
    fn reads_lower_case_and_unpadded_secrets() {
        // M0 issue K4: keepass-rs rejects lower-case Base32
        let upper = Totp::parse_uri("otpauth://totp/a?secret=JBSWY3DPEHPK3PXP").unwrap();
        let lower = Totp::parse_uri("otpauth://totp/a?secret=jbswy3dpehpk3pxp").unwrap();
        let spaced = Totp::parse_uri("otpauth://totp/a?secret=JBSW%20Y3DP%20EHPK%203PXP").unwrap();
        for t in [0, 59, 1_700_000_000] {
            assert_eq!(upper.code_at(t), lower.code_at(t));
            assert_eq!(upper.code_at(t), spaced.code_at(t));
        }
        assert!(Totp::parse_uri("otpauth://totp/u?secret=MZXW6YTBOI").is_ok());
    }

    #[test]
    fn honours_period_digits_and_algorithm() {
        let totp = Totp::parse_uri(&format!(
            "otpauth://totp/Example:bob?secret={}&period=60&digits=8&issuer=Example&algorithm=SHA256",
            base32(SHA256_SECRET.as_bytes())
        ))
        .unwrap();
        let c = totp.code_at(119);
        assert_eq!(c.period, 60);
        assert_eq!(c.remaining, 1);
        // counter 1 with a 60 s period is the same as t = 59 with 30 s
        assert_eq!(c.code, "46119246");
    }

    #[test]
    fn rejects_what_it_cannot_use() {
        for bad in [
            "otpauth://hotp/a?secret=JBSWY3DPEHPK3PXP&counter=1",
            "otpauth://totp/a?secret=not*base32",
            "otpauth://totp/a",
            "otpauth://totp/a?secret=JBSWY3DPEHPK3PXP&digits=3",
            "otpauth://totp/a?secret=JBSWY3DPEHPK3PXP&period=0",
            "otpauth://totp/a?secret=JBSWY3DPEHPK3PXP&algorithm=MD5",
            "just some text",
        ] {
            assert_eq!(
                Totp::parse_otp(bad).unwrap_err(),
                TotpError::Invalid,
                "{bad}"
            );
        }
        assert_eq!(from(&fields(&[])).unwrap_err(), TotpError::Missing);
    }

    #[test]
    fn reads_keepass_2_native_fields() {
        let expect = |map: HashMap<String, String>, t: u64, code: &str| {
            assert_eq!(from(&map).unwrap().code_at(t).code, code, "{map:?}");
        };
        expect(
            fields(&[("TimeOtp-Secret", SHA1_SECRET), ("TimeOtp-Length", "8")]),
            59,
            "94287082",
        );
        let hex: String = SHA1_SECRET.bytes().map(|b| format!("{b:02x}")).collect();
        expect(
            fields(&[("TimeOtp-Secret-Hex", &hex), ("TimeOtp-Length", "8")]),
            59,
            "94287082",
        );
        let b64 = B64.encode(SHA1_SECRET);
        expect(fields(&[("TimeOtp-Secret-Base64", &b64)]), 59, "287082");
        expect(
            fields(&[
                ("TimeOtp-Secret-Base32", &base32(SHA256_SECRET.as_bytes())),
                ("TimeOtp-Length", "8"),
                ("TimeOtp-Algorithm", "HMAC-SHA-256"),
            ]),
            59,
            "46119246",
        );
        expect(
            fields(&[
                ("TimeOtp-Secret-Base32", &base32(SHA1_SECRET.as_bytes())),
                ("TimeOtp-Period", "60"),
            ]),
            119,
            "287082",
        );
        assert_eq!(
            from(&fields(&[("TimeOtp-Secret-Base32", "!!")])).unwrap_err(),
            TotpError::Invalid
        );
    }

    #[test]
    fn reads_legacy_and_keeotp_fields() {
        let seed = base32(SHA1_SECRET.as_bytes());
        let legacy = fields(&[("TOTP Seed", &seed), ("TOTP Settings", "30;8")]);
        assert_eq!(from(&legacy).unwrap().code_at(59).code, "94287082");
        let keeotp = fields(&[("otp", &format!("key={seed}&size=8&step=30"))]);
        assert_eq!(from(&keeotp).unwrap().code_at(59).code, "94287082");
        // `otp` wins over the other forms, as in KeePassXC
        let both = fields(&[
            ("otp", &format!("otpauth://totp/a?secret={seed}")),
            ("TOTP Seed", "AAAA"),
        ]);
        assert_eq!(from(&both).unwrap().code_at(59).code, "287082");
    }

    #[test]
    fn steam_codes_use_the_steam_alphabet() {
        let seed = base32(SHA1_SECRET.as_bytes());
        let by_encoder =
            Totp::parse_uri(&format!("otpauth://totp/a?secret={seed}&encoder=steam")).unwrap();
        let by_issuer = Totp::parse_uri(&format!(
            "otpauth://totp/Steam:me?secret={seed}&issuer=Steam"
        ))
        .unwrap();
        let legacy = from(&fields(&[("TOTP Seed", &seed), ("TOTP Settings", "30;S")])).unwrap();
        let code = by_encoder.code_at(59).code;
        assert_eq!(code.len(), 5);
        assert!(code.bytes().all(|b| STEAM_ALPHABET.contains(&b)));
        assert_eq!(by_issuer.code_at(59).code, code);
        assert_eq!(legacy.code_at(59).code, code);
    }

    #[test]
    fn never_prints_the_secret() {
        let totp = Totp::parse_uri("otpauth://totp/a?secret=JBSWY3DPEHPK3PXP").unwrap();
        let text = format!("{totp:?}");
        assert!(!text.contains("JBSW") && !text.contains("secret:"));
    }
}
