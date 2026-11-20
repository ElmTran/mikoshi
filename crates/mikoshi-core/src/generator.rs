//! The password generator (features.md §3.9).
//!
//! - Random numbers come from the operating system (`getrandom`), and every
//!   pick is unbiased (rejection sampling, never `% n`).
//! - Each character class that is turned on appears at least once; the
//!   positions are shuffled afterwards so the guaranteed ones are not first.
//! - Passphrases use the EFF large word list (7776 words, 12.9 bits each).
//!   The list is by the Electronic Frontier Foundation, CC BY 3.0 US:
//!   <https://www.eff.org/dice>.
//! - Strength is estimated with zxcvbn on this side, so its dictionaries are
//!   never loaded into the UI.

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::Secret;

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
/// ASCII punctuation, as KeePassXC's "special characters"
const SYMBOLS: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";
/// Characters easily mistaken for one another
const LOOK_ALIKE: &str = "Il1|O0";

pub const MIN_LENGTH: u32 = 4;
pub const MAX_LENGTH: u32 = 128;
pub const MIN_WORDS: u32 = 3;
pub const MAX_WORDS: u32 = 12;

const WORDLIST: &str = include_str!("eff_large_wordlist.txt");

fn words() -> impl Iterator<Item = &'static str> {
    WORDLIST.lines().filter_map(|l| l.split('\t').nth(1))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Password,
    Passphrase,
}

/// What to generate. Kept in the settings file (only these choices, never a
/// result).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneratorOptions {
    pub mode: Mode,
    pub length: u32,
    pub lower: bool,
    pub upper: bool,
    pub digits: bool,
    pub symbols: bool,
    pub exclude_look_alike: bool,
    pub words: u32,
    pub separator: String,
    pub capitalize: bool,
    /// one digit appended to a random word
    pub add_digit: bool,
}

impl Default for GeneratorOptions {
    fn default() -> Self {
        GeneratorOptions {
            mode: Mode::Password,
            length: 20,
            lower: true,
            upper: true,
            digits: true,
            symbols: true,
            exclude_look_alike: false,
            words: 5,
            separator: "-".into(),
            capitalize: false,
            add_digit: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenError {
    /// no character class is turned on
    NoCharacters,
}

/// A generated password and how much randomness went into it.
pub struct Generated {
    pub password: Secret,
    /// bits of entropy of the generator with these options (what an attacker
    /// who knows the options faces)
    pub bits: f64,
}

/// A number in `0..n`, every value equally likely.
fn below(n: u32) -> u32 {
    assert!(n > 0);
    // the largest multiple of n that fits; draws at or above it are retried
    let zone = u32::MAX - (u32::MAX % n);
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).expect("the operating system's random number generator failed");
        let r = u32::from_le_bytes(b);
        if r < zone {
            return r % n;
        }
    }
}

fn pick<T: Copy>(from: &[T]) -> T {
    from[below(from.len() as u32) as usize]
}

/// Fisher–Yates with unbiased draws.
fn shuffle<T>(v: &mut [T]) {
    for i in (1..v.len()).rev() {
        v.swap(i, below(i as u32 + 1) as usize);
    }
}

impl GeneratorOptions {
    /// The options with every number in range and the separator tidied.
    pub fn clamped(&self) -> GeneratorOptions {
        let mut o = self.clone();
        o.length = o.length.clamp(MIN_LENGTH, MAX_LENGTH);
        o.words = o.words.clamp(MIN_WORDS, MAX_WORDS);
        o.separator = o
            .separator
            .chars()
            .filter(|c| !c.is_control())
            .take(3)
            .collect();
        o
    }

    /// The character classes turned on, look-alikes removed when asked.
    fn classes(&self) -> Vec<Vec<char>> {
        let keep = |set: &str| -> Vec<char> {
            set.chars()
                .filter(|c| !(self.exclude_look_alike && LOOK_ALIKE.contains(*c)))
                .collect()
        };
        [
            (self.lower, LOWER),
            (self.upper, UPPER),
            (self.digits, DIGITS),
            (self.symbols, SYMBOLS),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, set)| keep(set))
        .collect()
    }
}

/// Generates a password or passphrase.
pub fn generate(options: &GeneratorOptions) -> Result<Generated, GenError> {
    let o = options.clamped();
    match o.mode {
        Mode::Password => {
            let classes = o.classes();
            if classes.is_empty() {
                return Err(GenError::NoCharacters);
            }
            let all: Vec<char> = classes.concat();
            let mut chars: Zeroizing<Vec<char>> =
                Zeroizing::new(Vec::with_capacity(o.length as usize));
            for c in &classes {
                chars.push(pick(c));
            }
            while chars.len() < o.length as usize {
                chars.push(pick(&all));
            }
            shuffle(&mut chars);
            // bits for the free choices; the guaranteed ones count as picks
            // from their own class (a slight overestimate is avoided by
            // using the smaller set)
            let bits = classes.iter().map(|c| (c.len() as f64).log2()).sum::<f64>()
                + (o.length as usize - classes.len()) as f64 * (all.len() as f64).log2();
            Ok(Generated {
                password: Secret::new(chars.iter().collect::<String>()),
                bits,
            })
        }
        Mode::Passphrase => {
            let list: Vec<&str> = words().collect();
            let mut picked: Vec<String> = (0..o.words)
                .map(|_| {
                    let w = pick(&list);
                    if o.capitalize {
                        let mut c = w.chars();
                        c.next()
                            .map(|f| f.to_uppercase().chain(c).collect())
                            .unwrap_or_default()
                    } else {
                        w.to_string()
                    }
                })
                .collect();
            let mut bits = o.words as f64 * (list.len() as f64).log2();
            if o.add_digit {
                let i = below(o.words) as usize;
                picked[i].push(char::from(b'0' + below(10) as u8));
                bits += 10f64.log2() + (o.words as f64).log2();
            }
            let text = Zeroizing::new(picked.join(&o.separator));
            for w in &mut picked {
                zeroize::Zeroize::zeroize(w);
            }
            Ok(Generated {
                password: Secret::new(text.as_str()),
                bits,
            })
        }
    }
}

/// How hard a password is to guess, by zxcvbn.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Strength {
    /// 0 (guessed in under 10³ tries) to 4 (over 10¹⁰)
    pub score: u8,
    /// log10 of the estimated number of guesses
    pub guesses_log10: f64,
}

pub fn strength(password: &str) -> Strength {
    if password.is_empty() {
        return Strength {
            score: 0,
            guesses_log10: 0.0,
        };
    }
    // zxcvbn slows down sharply on very long input and learns nothing more
    let cut: String = password.chars().take(256).collect();
    let e = zxcvbn::zxcvbn(&cut, &[]);
    Strength {
        score: u8::from(e.score()),
        guesses_log10: e.guesses_log10(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gen_pw(o: &GeneratorOptions) -> String {
        generate(o).unwrap().password.expose().to_string()
    }

    #[test]
    fn the_word_list_is_complete() {
        let w: Vec<&str> = words().collect();
        assert_eq!(w.len(), 7776);
        assert_eq!(w.first(), Some(&"abacus"));
        assert_eq!(w.last(), Some(&"zoom"));
    }

    #[test]
    fn passwords_follow_the_options() {
        let mut o = GeneratorOptions::default();
        for len in [4, 12, 20, 64, 128] {
            o.length = len;
            let p = gen_pw(&o);
            assert_eq!(p.chars().count(), len as usize);
            assert!(p.chars().any(|c| c.is_ascii_lowercase()));
            assert!(p.chars().any(|c| c.is_ascii_uppercase()));
            assert!(p.chars().any(|c| c.is_ascii_digit()));
            assert!(p.chars().any(|c| SYMBOLS.contains(c)));
        }
        o.length = 1000;
        assert_eq!(gen_pw(&o).len(), MAX_LENGTH as usize, "clamped");

        let only_digits = GeneratorOptions {
            lower: false,
            upper: false,
            symbols: false,
            ..GeneratorOptions::default()
        };
        assert!(gen_pw(&only_digits).chars().all(|c| c.is_ascii_digit()));

        let none = GeneratorOptions {
            lower: false,
            upper: false,
            digits: false,
            symbols: false,
            ..GeneratorOptions::default()
        };
        assert_eq!(generate(&none).err(), Some(GenError::NoCharacters));
    }

    #[test]
    fn look_alikes_can_be_left_out() {
        let o = GeneratorOptions {
            length: 128,
            exclude_look_alike: true,
            ..GeneratorOptions::default()
        };
        for _ in 0..50 {
            assert!(!gen_pw(&o).chars().any(|c| LOOK_ALIKE.contains(c)));
        }
    }

    #[test]
    fn characters_come_out_evenly() {
        // 26 lower-case letters, many draws: each letter's count should be
        // close to the mean (a biased `% n` would skew the first ones)
        let o = GeneratorOptions {
            length: 128,
            upper: false,
            digits: false,
            symbols: false,
            ..GeneratorOptions::default()
        };
        let mut counts = [0u32; 26];
        for _ in 0..400 {
            for c in gen_pw(&o).bytes() {
                counts[(c - b'a') as usize] += 1;
            }
        }
        let n: u32 = counts.iter().sum();
        let mean = n as f64 / 26.0;
        // chi-square with 25 degrees of freedom; 60 is far beyond p = 0.001
        let chi: f64 = counts
            .iter()
            .map(|&c| (c as f64 - mean).powi(2) / mean)
            .sum();
        assert!(chi < 60.0, "chi-square {chi}");
    }

    #[test]
    fn passphrases_follow_the_options() {
        let list: std::collections::HashSet<&str> = words().collect();
        let o = GeneratorOptions {
            mode: Mode::Passphrase,
            words: 6,
            separator: " ".into(),
            ..GeneratorOptions::default()
        };
        let g = generate(&o).unwrap();
        let parts: Vec<&str> = g.password.expose().split(' ').collect();
        assert_eq!(parts.len(), 6);
        assert!(parts.iter().all(|w| list.contains(w)));
        assert!((g.bits - 6.0 * 7776f64.log2()).abs() < 1e-9);

        let o = GeneratorOptions {
            mode: Mode::Passphrase,
            words: 4,
            separator: "_".into(),
            capitalize: true,
            add_digit: true,
            ..GeneratorOptions::default()
        };
        let p = gen_pw(&o);
        let parts: Vec<&str> = p.split('_').collect();
        assert_eq!(parts.len(), 4);
        assert!(
            parts
                .iter()
                .all(|w| w.chars().next().unwrap().is_ascii_uppercase())
        );
        assert_eq!(p.chars().filter(|c| c.is_ascii_digit()).count(), 1);
    }

    #[test]
    fn strength_is_sensible() {
        assert_eq!(strength("").score, 0);
        assert_eq!(strength("password").score, 0);
        assert!(strength("correct horse battery staple").score >= 3);
        let strong = gen_pw(&GeneratorOptions::default());
        assert_eq!(strength(&strong).score, 4);
        assert!(
            strength(&"a".repeat(10_000)).guesses_log10 >= 0.0,
            "long input is cut"
        );
    }

    #[test]
    fn bits_match_the_options() {
        let o = GeneratorOptions {
            length: 10,
            upper: false,
            symbols: false,
            ..GeneratorOptions::default()
        };
        let b = generate(&o).unwrap().bits;
        // 2 guaranteed (26 and 10 choices) + 8 free from 36
        let expected = 26f64.log2() + 10f64.log2() + 8.0 * 36f64.log2();
        assert!((b - expected).abs() < 1e-9);
    }
}
