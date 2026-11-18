//! Auto-type (features.md §3.10): which window an entry belongs to, and what
//! its keystroke sequence says. Nothing here presses keys or reads secrets:
//! a parsed sequence names the fields it needs, and the typing side (in
//! src-tauri) fetches their values only when it types.

use regex::RegexBuilder;

/// Used when neither the window association, the entry nor its groups give
/// one (KeePass's default).
pub const DEFAULT_SEQUENCE: &str = "{USERNAME}{TAB}{PASSWORD}{ENTER}";

/// Whether a window association fits a window title. Case-insensitive.
///
/// - `//…//` is a regular expression, searched anywhere in the title
/// - anything else must match the whole title, `*` standing for any text
///   (`*Notepad*`, `Login - Example*`)
pub fn window_matches(pattern: &str, title: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if let Some(re) = pattern
        .strip_prefix("//")
        .and_then(|p| p.strip_suffix("//"))
        .filter(|p| !p.is_empty())
    {
        // a broken expression matches nothing rather than everything
        return RegexBuilder::new(re)
            .case_insensitive(true)
            .size_limit(1 << 20)
            .build()
            .is_ok_and(|r| r.is_match(title));
    }
    wildcard(&pattern.to_lowercase(), &title.to_lowercase())
}

/// Whether a browser page belongs to an entry: the page's host is the entry
/// URL's host or one of its subdomains (`accounts.example.com` for
/// `example.com`). A leading `www.` on the entry is ignored, so
/// `www.example.com` covers `example.com` too. Hosts without a dot
/// (`localhost`) and IP addresses must be equal. Only web addresses count:
/// http, https, or no scheme at all.
pub fn url_matches(entry_url: &str, page_url: &str) -> bool {
    let (Some(entry), Some(page)) = (web_host(entry_url), web_host(page_url)) else {
        return false;
    };
    let entry = entry.strip_prefix("www.").unwrap_or(&entry);
    if entry.is_empty() {
        return false;
    }
    let exact_only = !entry.contains('.')
        || entry.starts_with('[')
        || entry.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    page == entry || (!exact_only && page.ends_with(&format!(".{entry}")))
}

/// The host of a web address, lower case, without user, port or trailing
/// dot. `None` for other schemes (`cmd://`, `ftp://`) and for text that has
/// no host.
fn web_host(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = match url.split_once("://") {
        Some((scheme, rest)) => {
            let scheme = scheme.to_ascii_lowercase();
            (scheme == "http" || scheme == "https").then_some(rest)?
        }
        // no scheme: "example.com/login", as Chrome shows it
        None => url,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host_port.starts_with('[') {
        // IPv6: keep the brackets, drop the port after them
        &host_port[..=host_port.find(']')?]
    } else {
        host_port.split(':').next()?
    };
    let host = host.trim_end_matches('.').to_lowercase();
    // something like a host: no spaces, not empty
    (!host.is_empty() && !host.contains(char::is_whitespace)).then_some(host)
}

/// `*` matches any run of characters; the rest must be equal.
fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // classic greedy matching with backtracking to the last `*`
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Modifier keys held while a key is pressed (`+` Shift, `^` Ctrl, `%` Alt).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Mods {
    pub fn any(&self) -> bool {
        self.shift || self.ctrl || self.alt
    }
}

/// A key pressed by name (`{TAB}`) or with modifiers (`^a`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Tab,
    Enter,
    Space,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Escape,
    /// F1–F24
    F(u8),
    /// a character key, pressed rather than typed: only with modifiers
    Char(char),
}

/// One step of a parsed sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// typed as it is
    Text(String),
    /// a field's value: `Title`, `UserName`, `Password`, `URL`, `Notes`, or a
    /// custom field (`{S:name}`)
    Field(String),
    /// the current TOTP code
    Totp,
    Key(Key, Mods),
    /// `{DELAY n}`: wait n milliseconds
    Delay(u32),
    /// `{DELAY=n}`: from here on, n milliseconds between keys
    KeyDelay(u32),
}

/// Why a sequence cannot be typed. Nothing is typed then: half a login is
/// worse than none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceError {
    /// `{…}` that is not a known placeholder; carries the name (from the
    /// sequence, not a secret)
    Unknown(String),
    /// `{` without `}`, or `(` without `)`
    Unclosed,
    /// `)` without `(`
    Unopened,
    /// `+`, `^` or `%` before a field, a delay or the end
    DanglingModifier,
    /// a repeat count or delay that is not a number, or too large
    BadNumber,
}

/// Longest `{DELAY n}` accepted, and most repeats of `{TAB n}`: a typo must
/// not hang the typing for minutes.
const MAX_DELAY_MS: u32 = 10_000;
const MAX_REPEAT: u32 = 100;

/// Parses a KeePass auto-type sequence.
///
/// Placeholders (case-insensitive): `{USERNAME}` `{PASSWORD}` `{URL}`
/// `{TITLE}` `{NOTES}` `{TOTP}` `{S:name}`; keys `{TAB}` `{ENTER}` `{SPACE}`
/// `{BACKSPACE}`/`{BS}`/`{BKSP}` `{DELETE}`/`{DEL}` `{INSERT}`/`{INS}`
/// `{HOME}` `{END}` `{PGUP}` `{PGDN}` `{UP}` `{DOWN}` `{LEFT}` `{RIGHT}`
/// `{ESC}` `{F1}`–`{F24}`, a key with a count repeats (`{TAB 2}`);
/// `{DELAY n}` and `{DELAY=n}`. `~` is Enter. `+` `^` `%` hold Shift, Ctrl,
/// Alt for the next key or the next `(…)` group. `{+}` `{^}` `{%}` `{~}`
/// `{(}` `{)}` `{{}` `{}}` type the character itself.
pub fn parse(sequence: &str) -> Result<Vec<Step>, SequenceError> {
    let mut p = Parser {
        chars: sequence.chars().collect(),
        pos: 0,
        steps: Vec::new(),
    };
    p.run(Mods::default(), false)?;
    Ok(p.steps)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    steps: Vec<Step>,
}

impl Parser {
    /// Parses until the end, or until `)` when `in_group`. Every key inside a
    /// group gets `group_mods`.
    fn run(&mut self, group_mods: Mods, in_group: bool) -> Result<(), SequenceError> {
        let mut pending = Mods::default();
        while let Some(&c) = self.chars.get(self.pos) {
            self.pos += 1;
            let mods = merge(group_mods, pending);
            match c {
                '+' => pending.shift = true,
                '^' => pending.ctrl = true,
                '%' => pending.alt = true,
                '(' => {
                    self.run(mods, true)?;
                    pending = Mods::default();
                }
                ')' if in_group => {
                    if pending.any() {
                        return Err(SequenceError::DanglingModifier);
                    }
                    return Ok(());
                }
                ')' => return Err(SequenceError::Unopened),
                '~' => {
                    self.key(Key::Enter, mods, 1);
                    pending = Mods::default();
                }
                '{' => {
                    let body = self.placeholder()?;
                    self.braced(&body, mods)?;
                    pending = Mods::default();
                }
                c => {
                    if mods.any() {
                        self.key(Key::Char(c), mods, 1);
                    } else {
                        self.text(c);
                    }
                    pending = Mods::default();
                }
            }
        }
        if in_group {
            return Err(SequenceError::Unclosed);
        }
        if pending.any() {
            return Err(SequenceError::DanglingModifier);
        }
        Ok(())
    }

    /// The text between `{` and the matching `}`. `{}}` is the brace itself.
    fn placeholder(&mut self) -> Result<String, SequenceError> {
        let rest = &self.chars[self.pos..];
        // `{}}` and `{{}`: the first `}` after a one-character body
        let len = if rest.first() == Some(&'}') && rest.get(1) == Some(&'}') {
            1
        } else {
            rest.iter()
                .position(|&c| c == '}')
                .ok_or(SequenceError::Unclosed)?
        };
        let body: String = rest[..len].iter().collect();
        self.pos += len + 1;
        Ok(body)
    }

    fn braced(&mut self, body: &str, mods: Mods) -> Result<(), SequenceError> {
        // a single special character, typed literally
        let mut cs = body.chars();
        if let (Some(c), None) = (cs.next(), cs.next())
            && "+^%~(){}[]".contains(c)
        {
            if mods.any() {
                self.key(Key::Char(c), mods, 1);
            } else {
                self.text(c);
            }
            return Ok(());
        }

        if let Some(name) = body.strip_prefix("S:").or_else(|| body.strip_prefix("s:")) {
            return self.field(name.to_string(), mods);
        }
        let upper = body.to_uppercase();
        if let Some(n) = upper
            .strip_prefix("DELAY=")
            .or_else(|| upper.strip_prefix("DELAY "))
        {
            if mods.any() {
                return Err(SequenceError::DanglingModifier);
            }
            let ms = number(n, MAX_DELAY_MS)?;
            self.steps.push(if upper.starts_with("DELAY=") {
                Step::KeyDelay(ms)
            } else {
                Step::Delay(ms)
            });
            return Ok(());
        }
        let field = match upper.as_str() {
            "USERNAME" => Some("UserName"),
            "PASSWORD" => Some("Password"),
            "URL" => Some("URL"),
            "TITLE" => Some("Title"),
            "NOTES" => Some("Notes"),
            _ => None,
        };
        if let Some(f) = field {
            return self.field(f.to_string(), mods);
        }
        if upper == "TOTP" {
            if mods.any() {
                return Err(SequenceError::DanglingModifier);
            }
            self.steps.push(Step::Totp);
            return Ok(());
        }
        // a named key, optionally with a repeat count: {TAB 3}
        let (name, count) = match upper.split_once(' ') {
            Some((n, c)) => (n, number(c, MAX_REPEAT)?),
            None => (upper.as_str(), 1),
        };
        let key = match name {
            "TAB" => Key::Tab,
            "ENTER" => Key::Enter,
            "SPACE" => Key::Space,
            "BACKSPACE" | "BS" | "BKSP" => Key::Backspace,
            "DELETE" | "DEL" => Key::Delete,
            "INSERT" | "INS" => Key::Insert,
            "HOME" => Key::Home,
            "END" => Key::End,
            "PGUP" => Key::PageUp,
            "PGDN" => Key::PageDown,
            "UP" => Key::Up,
            "DOWN" => Key::Down,
            "LEFT" => Key::Left,
            "RIGHT" => Key::Right,
            "ESC" | "ESCAPE" => Key::Escape,
            f if f.starts_with('F') => match f[1..].parse::<u8>() {
                Ok(n @ 1..=24) => Key::F(n),
                _ => return Err(SequenceError::Unknown(body.to_string())),
            },
            _ => return Err(SequenceError::Unknown(body.to_string())),
        };
        self.key(key, mods, count);
        Ok(())
    }

    fn field(&mut self, name: String, mods: Mods) -> Result<(), SequenceError> {
        if mods.any() {
            return Err(SequenceError::DanglingModifier);
        }
        self.steps.push(Step::Field(name));
        Ok(())
    }

    fn key(&mut self, key: Key, mods: Mods, count: u32) {
        for _ in 0..count {
            self.steps.push(Step::Key(key, mods));
        }
    }

    /// Joins consecutive characters into one text step.
    fn text(&mut self, c: char) {
        if let Some(Step::Text(t)) = self.steps.last_mut() {
            t.push(c);
        } else {
            self.steps.push(Step::Text(c.to_string()));
        }
    }
}

fn merge(a: Mods, b: Mods) -> Mods {
    Mods {
        shift: a.shift || b.shift,
        ctrl: a.ctrl || b.ctrl,
        alt: a.alt || b.alt,
    }
}

fn number(text: &str, max: u32) -> Result<u32, SequenceError> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|&n| n <= max)
        .ok_or(SequenceError::BadNumber)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods {
        shift: false,
        ctrl: false,
        alt: false,
    };
    const CTRL: Mods = Mods {
        shift: false,
        ctrl: true,
        alt: false,
    };

    fn field(f: &str) -> Step {
        Step::Field(f.to_string())
    }
    fn text(t: &str) -> Step {
        Step::Text(t.to_string())
    }

    #[test]
    fn wildcards_match_the_whole_title() {
        assert!(window_matches("*Notepad*", "notes.txt - Notepad"));
        assert!(window_matches("Login - Example*", "login - example - Edge"));
        assert!(window_matches("Exact Title", "exact title"));
        assert!(!window_matches("Exact Title", "Exact Title 2"));
        assert!(!window_matches("Login*", "My Login"));
        assert!(window_matches("*a*b*", "xxaxxbxx"));
        assert!(window_matches("*", "anything"));
        assert!(!window_matches("", "anything"));
        assert!(!window_matches("   ", "anything"));
    }

    #[test]
    fn regex_associations_search_the_title() {
        assert!(window_matches("//^Bank .*$//", "bank of example - Chrome"));
        assert!(!window_matches("//^Bank .*$//", "My Bank"));
        assert!(window_matches("//git(hub|lab)//", "Sign in to GitLab"));
        // broken expressions match nothing
        assert!(!window_matches("//(unclosed//", "(unclosed"));
        // `////` is not an empty expression matching everything
        assert!(!window_matches("////", "anything"));
    }

    #[test]
    fn urls_match_the_host_and_its_subdomains() {
        let entry = "https://example.com/login?a=1&b=2";
        assert!(url_matches(entry, "https://example.com/other"));
        assert!(url_matches(entry, "example.com"));
        assert!(url_matches(entry, "accounts.example.com/signin"));
        assert!(url_matches(entry, "HTTPS://User@Example.COM.:8443/x"));
        assert!(!url_matches(entry, "notexample.com"));
        assert!(!url_matches(entry, "example.org"));
        assert!(!url_matches(entry, "example.com.evil.net"));
        // the entry's own subdomain does not cover the parent
        assert!(!url_matches("https://accounts.example.com", "example.com"));
        // www. on the entry is ignored
        assert!(url_matches("www.example.com", "example.com"));
        assert!(url_matches("http://www.example.com", "login.example.com"));
        // no dot, IP addresses: exact only
        assert!(url_matches("http://localhost:3000", "localhost/app"));
        assert!(!url_matches("http://localhost", "x.localhost"));
        assert!(url_matches("http://10.0.0.1/admin", "10.0.0.1"));
        assert!(!url_matches("http://0.0.1", "10.0.0.1"));
        assert!(url_matches("http://[::1]:8080/", "[::1]"));
        // only web addresses
        assert!(!url_matches("cmd://notepad.exe", "notepad.exe"));
        assert!(!url_matches(entry, "ftp://example.com"));
        assert!(!url_matches("", "example.com"));
        assert!(!url_matches(entry, ""));
        assert!(!url_matches("https://", "example.com"));
    }

    #[test]
    fn parses_the_default_sequence() {
        assert_eq!(
            parse(DEFAULT_SEQUENCE).unwrap(),
            vec![
                field("UserName"),
                Step::Key(Key::Tab, NONE),
                field("Password"),
                Step::Key(Key::Enter, NONE),
            ]
        );
    }

    #[test]
    fn parses_placeholders_case_insensitively() {
        assert_eq!(
            parse("{username}{Url}{TITLE}{notes}{totp}{S:Secret Field}{s:pin}").unwrap(),
            vec![
                field("UserName"),
                field("URL"),
                field("Title"),
                field("Notes"),
                Step::Totp,
                field("Secret Field"),
                field("pin"),
            ]
        );
    }

    #[test]
    fn parses_keys_repeats_and_delays() {
        assert_eq!(
            parse("{TAB 2}{bs}{F12}{DELAY 100}{DELAY=25}~").unwrap(),
            vec![
                Step::Key(Key::Tab, NONE),
                Step::Key(Key::Tab, NONE),
                Step::Key(Key::Backspace, NONE),
                Step::Key(Key::F(12), NONE),
                Step::Delay(100),
                Step::KeyDelay(25),
                Step::Key(Key::Enter, NONE),
            ]
        );
    }

    #[test]
    fn escapes_type_the_character() {
        assert_eq!(
            parse("a{+}{^}{%}{~}{(}{)}{{}{}}b").unwrap(),
            vec![text("a+^%~(){}b")]
        );
    }

    #[test]
    fn modifiers_apply_to_the_next_key_or_group() {
        let shift = Mods {
            shift: true,
            ..NONE
        };
        let ctrl_alt = Mods { alt: true, ..CTRL };
        assert_eq!(
            parse("^a{DEL}").unwrap(),
            vec![
                Step::Key(Key::Char('a'), CTRL),
                Step::Key(Key::Delete, NONE)
            ]
        );
        assert_eq!(
            parse("+{TAB}x").unwrap(),
            vec![Step::Key(Key::Tab, shift), text("x")]
        );
        assert_eq!(
            parse("^%(ab)c").unwrap(),
            vec![
                Step::Key(Key::Char('a'), ctrl_alt),
                Step::Key(Key::Char('b'), ctrl_alt),
                text("c"),
            ]
        );
    }

    #[test]
    fn refuses_what_it_cannot_type() {
        use SequenceError::*;
        assert_eq!(parse("{NOPE}"), Err(Unknown("NOPE".into())));
        assert_eq!(parse("{F25}"), Err(Unknown("F25".into())));
        assert_eq!(parse("{USERNAME"), Err(Unclosed));
        assert_eq!(parse("(abc"), Err(Unclosed));
        assert_eq!(parse("abc)"), Err(Unopened));
        assert_eq!(parse("abc^"), Err(DanglingModifier));
        assert_eq!(parse("^{PASSWORD}"), Err(DanglingModifier));
        assert_eq!(parse("{DELAY x}"), Err(BadNumber));
        assert_eq!(parse("{DELAY 999999}"), Err(BadNumber));
        assert_eq!(parse("{TAB 1000}"), Err(BadNumber));
    }

    #[test]
    fn an_empty_sequence_types_nothing() {
        assert_eq!(parse("").unwrap(), vec![]);
    }
}
