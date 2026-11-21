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
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    // A Chinese domain is "中文.com" in the address bar and "xn--fiq228c.com"
    // on the wire, and an entry may hold either: compare the ASCII form.
    if host.starts_with('[') || (host.is_ascii() && !host.contains("xn--")) {
        return Some(host);
    }
    Some(idna::domain_to_ascii(&host).unwrap_or(host))
}

/// A host as people read it: "xn--fiq228c.com" becomes "中文.com". Anything
/// that does not decode cleanly is returned as it is.
pub fn readable_host(host: &str) -> String {
    match idna::domain_to_unicode(host) {
        (unicode, Ok(())) => unicode,
        _ => host.to_string(),
    }
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
    /// `{WIN}` / `{LWIN}`
    Win,
    RightWin,
    /// the context-menu key
    Apps,
    CapsLock,
    NumLock,
    ScrollLock,
    PrintScreen,
    Break,
    Help,
    /// the numpad's `+` `-` `*` `/`
    NumAdd,
    NumSubtract,
    NumMultiply,
    NumDivide,
    /// numpad 0–9
    Numpad(u8),
    /// `{VKEY n}`: a virtual-key code. `extended`: `None` decides by the
    /// key, `{VKEY-EX n}` forces the extended flag on, `{VKEY-NX n}` off.
    Vk {
        code: u8,
        extended: Option<bool>,
    },
    /// a character key, pressed rather than typed: only with modifiers
    Char(char),
}

/// A part of the current date and time (`{DT_SIMPLE}`, `{DT_YEAR}`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePart {
    /// `yyyyMMddHHmmss`
    Simple,
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

/// The same parts as numbers, for `DatePart::format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

impl DatePart {
    /// KeePass's formats: four-digit year, everything else two digits.
    pub fn format(self, t: DateTime) -> String {
        match self {
            DatePart::Simple => format!(
                "{:04}{:02}{:02}{:02}{:02}{:02}",
                t.year, t.month, t.day, t.hour, t.minute, t.second
            ),
            DatePart::Year => format!("{:04}", t.year),
            DatePart::Month => format!("{:02}", t.month),
            DatePart::Day => format!("{:02}", t.day),
            DatePart::Hour => format!("{:02}", t.hour),
            DatePart::Minute => format!("{:02}", t.minute),
            DatePart::Second => format!("{:02}", t.second),
        }
    }
}

/// A value worked out from the entry rather than read from one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Derived {
    /// `{GROUP}`: the name of the entry's group
    Group,
    /// `{GROUP_PATH}`: the groups below the root, joined with `.`
    GroupPath,
    /// `{UUID}`: 32 hex digits, upper case
    Uuid,
    /// `{URL:…}`: a part of the entry's URL
    Url(UrlPart),
}

/// The parts KeePass's `{URL:…}` placeholders name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlPart {
    /// `{URL:RMVSCM}` / `{URL:WITHOUTSCHEME}`: everything after `scheme://`
    WithoutScheme,
    /// `{URL:SCM}`
    Scheme,
    /// `{URL:HOST}`
    Host,
    /// `{URL:PORT}`: the port, or the scheme's usual one
    Port,
    /// `{URL:PATH}`: from the first `/` after the host, without `?…` `#…`
    Path,
    /// `{URL:QUERY}`: `?…` including the `?`
    Query,
    /// `{URL:FRAGMENT}`: `#…` including the `#`
    Fragment,
    /// `{URL:USERINFO}`: `user:password` before the `@`
    UserInfo,
    /// `{URL:USERNAME}`
    UserName,
    /// `{URL:PASSWORD}`
    Password,
}

impl UrlPart {
    /// This part of `url`, empty when the URL does not have it. No
    /// normalising: the text comes back as written.
    pub fn of(self, url: &str) -> String {
        let url = url.trim();
        let (scheme, rest) = match url.split_once("://") {
            Some((s, r))
                if !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) =>
            {
                (s, r)
            }
            _ => ("", url),
        };
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(authority_end);
        let (user_info, host_port) = match authority.rsplit_once('@') {
            Some((u, h)) => (u, h),
            None => ("", authority),
        };
        let (host, port) = if let Some(end) = host_port
            .strip_prefix('[')
            .and_then(|_| host_port.find(']'))
        {
            let (h, p) = host_port.split_at(end + 1);
            (h, p.strip_prefix(':').unwrap_or(""))
        } else {
            match host_port.rsplit_once(':') {
                Some((h, p)) => (h, p),
                None => (host_port, ""),
            }
        };
        let fragment_at = tail.find('#').unwrap_or(tail.len());
        let (before_fragment, fragment) = tail.split_at(fragment_at);
        let query_at = before_fragment.find('?').unwrap_or(before_fragment.len());
        let (path, query) = before_fragment.split_at(query_at);
        match self {
            UrlPart::WithoutScheme => rest.to_string(),
            UrlPart::Scheme => scheme.to_string(),
            UrlPart::Host => host.to_string(),
            UrlPart::Port => {
                if !port.is_empty() {
                    port.to_string()
                } else {
                    match scheme.to_ascii_lowercase().as_str() {
                        "http" => "80",
                        "https" => "443",
                        "ftp" => "21",
                        _ => "",
                    }
                    .to_string()
                }
            }
            UrlPart::Path => path.to_string(),
            UrlPart::Query => query.to_string(),
            UrlPart::Fragment => fragment.to_string(),
            UrlPart::UserInfo => user_info.to_string(),
            UrlPart::UserName => user_info
                .split_once(':')
                .map_or(user_info, |(u, _)| u)
                .to_string(),
            UrlPart::Password => user_info.split_once(':').map_or("", |(_, p)| p).to_string(),
        }
    }
}

/// One step of a parsed sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// typed as it is
    Text(String),
    /// a field's value: `Title`, `UserName`, `Password`, `URL`, `Notes`, or a
    /// custom field (`{S:name}`)
    Field(String),
    /// the current TOTP code (`{TOTP}`, KeePass's `{TIMEOTP}`)
    Totp,
    /// a value worked out from the entry: group, UUID, URL parts
    Derived(Derived),
    /// the current date and time, local or UTC
    Date {
        part: DatePart,
        utc: bool,
    },
    Key(Key, Mods),
    /// `{DELAY n}`: wait n milliseconds
    Delay(u32),
    /// `{DELAY=n}`: from here on, n milliseconds between keys
    KeyDelay(u32),
    /// `{BEEP f d}`: a tone of f Hz for d milliseconds
    Beep {
        hz: u32,
        ms: u32,
    },
    /// KeePassXC's `{MODE=VIRTUAL}`: from here on, characters are pressed as
    /// keys of the keyboard layout instead of sent as Unicode (for remote
    /// desktops and games that ignore Unicode input)
    VirtualMode,
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

/// Parses a KeePass auto-type sequence. Covers what KeePass 2 and KeePassXC
/// both use, minus what needs a dialog or runs programs.
///
/// Placeholders (case-insensitive):
/// - values: `{USERNAME}` `{PASSWORD}` `{URL}` `{TITLE}` `{NOTES}` `{S:name}`
///   `{TOTP}`/`{TIMEOTP}` `{GROUP}` `{GROUP_PATH}` `{UUID}`
///   `{URL:RMVSCM|WITHOUTSCHEME|SCM|HOST|PORT|PATH|QUERY|FRAGMENT|USERINFO|USERNAME|PASSWORD}`
///   `{DT_SIMPLE}` `{DT_YEAR}` … `{DT_SECOND}` and the `{DT_UTC_…}` forms
/// - keys: `{TAB}` `{ENTER}` `{SPACE}` `{BACKSPACE}`/`{BS}`/`{BKSP}`
///   `{DELETE}`/`{DEL}` `{INSERT}`/`{INS}` `{HOME}` `{END}` `{PGUP}` `{PGDN}`
///   `{UP}` `{DOWN}` `{LEFT}` `{RIGHT}` `{ESC}` `{F1}`–`{F24}` `{WIN}`/`{LWIN}`
///   `{RWIN}` `{APPS}` `{CAPSLOCK}` `{NUMLOCK}` `{SCROLLLOCK}` `{PRTSC}`
///   `{BREAK}` `{HELP}` `{ADD}` `{SUBTRACT}` `{MULTIPLY}` `{DIVIDE}`
///   `{NUMPAD0}`–`{NUMPAD9}`, `{VKEY n}` `{VKEY-EX n}` `{VKEY-NX n}`; a named
///   key with a count repeats (`{TAB 2}`)
/// - actions: `{CLEARFIELD}` `{DELAY n}` `{DELAY=n}` `{BEEP hz ms}`
///   `{MODE=VIRTUAL}`; `{C:comment}` types nothing
/// - characters: `~` is Enter. `+` `^` `%` hold Shift, Ctrl, Alt for the
///   next key or the next `(…)` group. `{+}` `{^}` `{%}` `{~}` `{(}` `{)}`
///   `{{}` `{}}` `{[}` `{]}` and `{PLUS}` `{CARET}` `{PERCENT}` `{TILDE}`
///   `{LEFTPAREN}` `{RIGHTPAREN}` `{LEFTBRACE}` `{RIGHTBRACE}` type the
///   character itself.
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
            self.literal(c, mods);
            return Ok(());
        }

        if let Some(name) = body.strip_prefix("S:").or_else(|| body.strip_prefix("s:")) {
            return self.field(name.to_string(), mods);
        }
        // `{C:…}`: a comment, typed as nothing
        if body.starts_with("C:") || body.starts_with("c:") {
            return no_mods(mods);
        }
        let upper = body.to_uppercase();
        if let Some(n) = upper
            .strip_prefix("DELAY=")
            .or_else(|| upper.strip_prefix("DELAY "))
        {
            no_mods(mods)?;
            let ms = number(n, MAX_DELAY_MS)?;
            self.steps.push(if upper.starts_with("DELAY=") {
                Step::KeyDelay(ms)
            } else {
                Step::Delay(ms)
            });
            return Ok(());
        }
        // the special characters by name
        let named = match upper.as_str() {
            "PLUS" => Some('+'),
            "PERCENT" => Some('%'),
            "CARET" => Some('^'),
            "TILDE" => Some('~'),
            "LEFTPAREN" => Some('('),
            "RIGHTPAREN" => Some(')'),
            "LEFTBRACE" => Some('{'),
            "RIGHTBRACE" => Some('}'),
            _ => None,
        };
        if let Some(c) = named {
            self.literal(c, mods);
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
        if let Some(step) = value_placeholder(&upper) {
            no_mods(mods)?;
            self.steps.push(step);
            return Ok(());
        }
        match upper.as_str() {
            // KeePassXC's way: from the start of the field select to its
            // end, then delete. Works in multi-line fields too.
            "CLEARFIELD" => {
                no_mods(mods)?;
                let ctrl = Mods {
                    ctrl: true,
                    ..Mods::default()
                };
                let ctrl_shift = Mods {
                    shift: true,
                    ..ctrl
                };
                self.key(Key::Home, ctrl, 1);
                self.key(Key::End, ctrl_shift, 1);
                self.key(Key::Backspace, Mods::default(), 1);
                return Ok(());
            }
            "MODE=VIRTUAL" => {
                no_mods(mods)?;
                self.steps.push(Step::VirtualMode);
                return Ok(());
            }
            _ => {}
        }
        if let Some(args) = upper.strip_prefix("BEEP ") {
            no_mods(mods)?;
            let (hz, ms) = args
                .trim()
                .split_once(' ')
                .ok_or(SequenceError::BadNumber)?;
            let hz = number(hz, 32_767)?;
            if hz < 37 {
                return Err(SequenceError::BadNumber);
            }
            let ms = number(ms, MAX_DELAY_MS)?;
            self.steps.push(Step::Beep { hz, ms });
            return Ok(());
        }
        for (prefix, extended) in [
            ("VKEY-EX ", Some(true)),
            ("VKEY-NX ", Some(false)),
            ("VKEY ", None),
        ] {
            if let Some(n) = upper.strip_prefix(prefix) {
                let code = virtual_key_code(n)?;
                self.key(Key::Vk { code, extended }, mods, 1);
                return Ok(());
            }
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
            "WIN" | "LWIN" => Key::Win,
            "RWIN" => Key::RightWin,
            "APPS" => Key::Apps,
            "CAPSLOCK" => Key::CapsLock,
            "NUMLOCK" => Key::NumLock,
            "SCROLLLOCK" => Key::ScrollLock,
            "PRTSC" => Key::PrintScreen,
            "BREAK" => Key::Break,
            "HELP" => Key::Help,
            "ADD" => Key::NumAdd,
            "SUBTRACT" => Key::NumSubtract,
            "MULTIPLY" => Key::NumMultiply,
            "DIVIDE" => Key::NumDivide,
            n if n.starts_with("NUMPAD") => match n["NUMPAD".len()..].parse::<u8>() {
                Ok(d @ 0..=9) if n.len() == "NUMPAD".len() + 1 => Key::Numpad(d),
                _ => return Err(SequenceError::Unknown(body.to_string())),
            },
            f if f.starts_with('F') => match f[1..].parse::<u8>() {
                Ok(n @ 1..=24) => Key::F(n),
                _ => return Err(SequenceError::Unknown(body.to_string())),
            },
            _ => return Err(SequenceError::Unknown(body.to_string())),
        };
        self.key(key, mods, count);
        Ok(())
    }

    /// A character typed as itself, or pressed when modifiers are held.
    fn literal(&mut self, c: char, mods: Mods) {
        if mods.any() {
            self.key(Key::Char(c), mods, 1);
        } else {
            self.text(c);
        }
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

/// Placeholders that stand for a value, not a key, cannot take `+` `^` `%`.
fn no_mods(mods: Mods) -> Result<(), SequenceError> {
    if mods.any() {
        Err(SequenceError::DanglingModifier)
    } else {
        Ok(())
    }
}

/// `{TIMEOTP}`, `{GROUP}`, `{UUID}`, `{URL:…}`, `{DT_…}` (upper case).
fn value_placeholder(upper: &str) -> Option<Step> {
    let derived = |d| Some(Step::Derived(d));
    match upper {
        "TOTP" | "TIMEOTP" => return Some(Step::Totp),
        "GROUP" => return derived(Derived::Group),
        "GROUP_PATH" => return derived(Derived::GroupPath),
        "UUID" => return derived(Derived::Uuid),
        _ => {}
    }
    if let Some(part) = upper.strip_prefix("URL:") {
        let part = match part {
            "RMVSCM" | "WITHOUTSCHEME" => UrlPart::WithoutScheme,
            "SCM" => UrlPart::Scheme,
            "HOST" => UrlPart::Host,
            "PORT" => UrlPart::Port,
            "PATH" => UrlPart::Path,
            "QUERY" => UrlPart::Query,
            "FRAGMENT" => UrlPart::Fragment,
            "USERINFO" => UrlPart::UserInfo,
            "USERNAME" => UrlPart::UserName,
            "PASSWORD" => UrlPart::Password,
            _ => return None,
        };
        return derived(Derived::Url(part));
    }
    let rest = upper.strip_prefix("DT_")?;
    let (utc, name) = match rest.strip_prefix("UTC_") {
        Some(n) => (true, n),
        None => (false, rest),
    };
    let part = match name {
        "SIMPLE" => DatePart::Simple,
        "YEAR" => DatePart::Year,
        "MONTH" => DatePart::Month,
        "DAY" => DatePart::Day,
        "HOUR" => DatePart::Hour,
        "MINUTE" => DatePart::Minute,
        "SECOND" => DatePart::Second,
        _ => return None,
    };
    Some(Step::Date { part, utc })
}

/// The number in `{VKEY n}`: decimal, or hex with `0x`. 1–254.
fn virtual_key_code(text: &str) -> Result<u8, SequenceError> {
    let text = text.trim();
    let n = match text.strip_prefix("0X") {
        Some(hex) => u8::from_str_radix(hex, 16).ok(),
        None => text.parse::<u8>().ok(),
    };
    n.filter(|&n| (1..=254).contains(&n))
        .ok_or(SequenceError::BadNumber)
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
        // a Chinese domain, as the address bar shows it or in its ASCII form
        let ascii = format!("xn--{}.com", "vcsx1ip8b8w4i");
        let unicode = readable_host(&ascii);
        assert_ne!(unicode, ascii);
        assert!(!unicode.is_ascii());
        assert!(url_matches(
            &format!("https://{ascii}"),
            &format!("{unicode}/login")
        ));
        assert!(url_matches(
            &format!("https://{unicode}"),
            &format!("https://{ascii}/")
        ));
        assert!(url_matches(
            &format!("https://{unicode}"),
            &format!("www.{unicode}")
        ));
        assert!(!url_matches(&format!("https://{unicode}"), "example.com"));
        assert_eq!(readable_host("example.com"), "example.com");
        assert_eq!(readable_host("xn--zz"), "xn--zz");
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

    #[test]
    fn clearfield_selects_everything_and_deletes_it() {
        let ctrl_shift = Mods {
            shift: true,
            ..CTRL
        };
        assert_eq!(
            parse("{CLEARFIELD}{USERNAME}").unwrap(),
            vec![
                Step::Key(Key::Home, CTRL),
                Step::Key(Key::End, ctrl_shift),
                Step::Key(Key::Backspace, NONE),
                field("UserName"),
            ]
        );
        assert_eq!(parse("{clearfield}").unwrap().len(), 3);
        assert_eq!(parse("^{CLEARFIELD}"), Err(SequenceError::DanglingModifier));
    }

    #[test]
    fn special_characters_by_name() {
        assert_eq!(
            parse(
                "{PLUS}{PERCENT}{CARET}{TILDE}{LEFTPAREN}{RIGHTPAREN}{LEFTBRACE}{RIGHTBRACE}{[}{]}"
            )
            .unwrap(),
            vec![text("+%^~(){}[]")]
        );
        assert_eq!(
            parse("^{PLUS}").unwrap(),
            vec![Step::Key(Key::Char('+'), CTRL)]
        );
    }

    #[test]
    fn more_named_keys() {
        assert_eq!(
            parse("{WIN}{LWIN}{RWIN}{APPS}{CAPSLOCK}{NUMLOCK}{SCROLLLOCK}{PRTSC}{BREAK}{HELP}")
                .unwrap(),
            [
                Key::Win,
                Key::Win,
                Key::RightWin,
                Key::Apps,
                Key::CapsLock,
                Key::NumLock,
                Key::ScrollLock,
                Key::PrintScreen,
                Key::Break,
                Key::Help,
            ]
            .map(|k| Step::Key(k, NONE))
        );
        assert_eq!(
            parse("{ADD}{SUBTRACT}{MULTIPLY}{DIVIDE}{NUMPAD0}{numpad9 2}").unwrap(),
            [
                Key::NumAdd,
                Key::NumSubtract,
                Key::NumMultiply,
                Key::NumDivide,
                Key::Numpad(0),
                Key::Numpad(9),
                Key::Numpad(9),
            ]
            .map(|k| Step::Key(k, NONE))
        );
        assert_eq!(
            parse("{NUMPAD10}"),
            Err(SequenceError::Unknown("NUMPAD10".into()))
        );
        assert_eq!(
            parse("{NUMPAD}"),
            Err(SequenceError::Unknown("NUMPAD".into()))
        );
    }

    #[test]
    fn virtual_key_codes() {
        let vk = |code, extended| Key::Vk { code, extended };
        assert_eq!(
            parse("{VKEY 13}^{VKEY 0x41}{VKEY-EX 45}{vkey-nx 0x2E}").unwrap(),
            vec![
                Step::Key(vk(13, None), NONE),
                Step::Key(vk(0x41, None), CTRL),
                Step::Key(vk(45, Some(true)), NONE),
                Step::Key(vk(0x2e, Some(false)), NONE),
            ]
        );
        assert_eq!(parse("{VKEY 0}"), Err(SequenceError::BadNumber));
        assert_eq!(parse("{VKEY 255}"), Err(SequenceError::BadNumber));
        assert_eq!(parse("{VKEY x}"), Err(SequenceError::BadNumber));
    }

    #[test]
    fn values_worked_out_from_the_entry() {
        assert_eq!(
            parse("{TIMEOTP}{GROUP}{group_path}{UUID}{URL:HOST}{url:rmvscm}{URL:WITHOUTSCHEME}")
                .unwrap(),
            vec![
                Step::Totp,
                Step::Derived(Derived::Group),
                Step::Derived(Derived::GroupPath),
                Step::Derived(Derived::Uuid),
                Step::Derived(Derived::Url(UrlPart::Host)),
                Step::Derived(Derived::Url(UrlPart::WithoutScheme)),
                Step::Derived(Derived::Url(UrlPart::WithoutScheme)),
            ]
        );
        assert_eq!(
            parse("{URL:NOPE}"),
            Err(SequenceError::Unknown("URL:NOPE".into()))
        );
        assert_eq!(parse("+{GROUP}"), Err(SequenceError::DanglingModifier));
    }

    #[test]
    fn dates() {
        assert_eq!(
            parse("{DT_SIMPLE}{dt_year}{DT_UTC_SECOND}").unwrap(),
            vec![
                Step::Date {
                    part: DatePart::Simple,
                    utc: false
                },
                Step::Date {
                    part: DatePart::Year,
                    utc: false
                },
                Step::Date {
                    part: DatePart::Second,
                    utc: true
                },
            ]
        );
        let t = DateTime {
            year: 2026,
            month: 1,
            day: 9,
            hour: 7,
            minute: 5,
            second: 3,
        };
        assert_eq!(DatePart::Simple.format(t), "20260109070503");
        assert_eq!(DatePart::Year.format(t), "2026");
        assert_eq!(DatePart::Month.format(t), "01");
        assert_eq!(DatePart::Second.format(t), "03");
        assert_eq!(
            parse("{DT_WEEK}"),
            Err(SequenceError::Unknown("DT_WEEK".into()))
        );
    }

    #[test]
    fn comments_beeps_and_virtual_mode() {
        assert_eq!(
            parse("a{C:log in}b{BEEP 800 200}{MODE=VIRTUAL}c").unwrap(),
            vec![
                text("ab"),
                Step::Beep { hz: 800, ms: 200 },
                Step::VirtualMode,
                text("c"),
            ]
        );
        assert_eq!(parse("{BEEP 10 200}"), Err(SequenceError::BadNumber));
        assert_eq!(parse("{BEEP 800}"), Err(SequenceError::BadNumber));
        assert_eq!(
            parse("{MODE=UNICODE}"),
            Err(SequenceError::Unknown("MODE=UNICODE".into()))
        );
    }

    #[test]
    fn url_parts_like_keepass() {
        let url = "https://user:pw@keepass.info:81/path/example.php?q=e&s=t#top";
        let part = |p: UrlPart| p.of(url);
        assert_eq!(
            part(UrlPart::WithoutScheme),
            "user:pw@keepass.info:81/path/example.php?q=e&s=t#top"
        );
        assert_eq!(part(UrlPart::Scheme), "https");
        assert_eq!(part(UrlPart::Host), "keepass.info");
        assert_eq!(part(UrlPart::Port), "81");
        assert_eq!(part(UrlPart::Path), "/path/example.php");
        assert_eq!(part(UrlPart::Query), "?q=e&s=t");
        assert_eq!(part(UrlPart::Fragment), "#top");
        assert_eq!(part(UrlPart::UserInfo), "user:pw");
        assert_eq!(part(UrlPart::UserName), "user");
        assert_eq!(part(UrlPart::Password), "pw");
        // the scheme's usual port
        assert_eq!(UrlPart::Port.of("https://example.com/x"), "443");
        assert_eq!(UrlPart::Port.of("http://example.com"), "80");
        assert_eq!(UrlPart::Port.of("example.com"), "");
        // no scheme
        assert_eq!(
            UrlPart::Host.of("dash.cloudflare.com/login"),
            "dash.cloudflare.com"
        );
        assert_eq!(UrlPart::Scheme.of("dash.cloudflare.com/login"), "");
        assert_eq!(UrlPart::Path.of("dash.cloudflare.com/login"), "/login");
        // IPv6
        assert_eq!(UrlPart::Host.of("http://[::1]:8080/a"), "[::1]");
        assert_eq!(UrlPart::Port.of("http://[::1]:8080/a"), "8080");
        assert_eq!(UrlPart::Host.of(""), "");
    }
}
