//! Search queries and ranking (features.md §3.4).
//!
//! A query is a list of words, all of which must match (case-insensitive).
//! A word may be limited to one field with a prefix: `title:`, `user:`,
//! `url:` or `tag:`; quotes keep spaces in a word (`title:"my bank"`).
//!
//! Each word is matched as a substring first and, failing that, fuzzily: its
//! letters in order with little in between (`gthb` finds "GitHub"). Better
//! matches rank first: title prefix, title, user name, URL, tags, notes, then
//! the fuzzy matches.

pub struct Haystack<'a> {
    pub title: &'a str,
    pub username: &'a str,
    pub url: &'a str,
    pub notes: &'a str,
    pub tags: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Any,
    Title,
    User,
    Url,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Term {
    scope: Scope,
    /// lower case
    text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<Term>,
}

/// How good a match is; lower is better.
pub type Score = u32;

// tiers, best first; a tier is worth more than any fuzzy penalty
const TITLE_PREFIX: Score = 0;
const TITLE: Score = 1;
const USER: Score = 2;
const URL: Score = 3;
const TAG: Score = 4;
const NOTES: Score = 5;
const FUZZY_TITLE: Score = 6;
const FUZZY_USER: Score = 7;
const FUZZY_URL: Score = 8;
const TIER: Score = 100;

fn scope_of(prefix: &str) -> Option<Scope> {
    match prefix.to_lowercase().as_str() {
        "title" => Some(Scope::Title),
        "user" | "username" => Some(Scope::User),
        "url" => Some(Scope::Url),
        "tag" | "tags" => Some(Scope::Tag),
        _ => None,
    }
}

/// Splits on whitespace, keeping quoted parts together and dropping quotes.
fn words(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in query.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Letters of `needle` in order inside `hay`, with few letters in between.
/// Returns the penalty (letters skipped, plus a little for a late start).
fn fuzzy(needle: &[char], hay: &str) -> Option<Score> {
    if needle.len() < 2 {
        return None;
    }
    let hay: Vec<char> = hay.to_lowercase().chars().collect();
    let mut best: Option<Score> = None;
    // try every place the first letter occurs, keep the tightest match
    for start in (0..hay.len()).filter(|&i| hay[i] == needle[0]) {
        let mut at = start;
        let mut ok = true;
        for &c in &needle[1..] {
            match hay[at + 1..].iter().position(|&h| h == c) {
                Some(p) => at += 1 + p,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            break;
        }
        let gaps = (at - start + 1 - needle.len()) as Score;
        // spread too thin to mean anything
        if gaps > 2 * needle.len() as Score {
            continue;
        }
        let penalty = gaps * 4 + (start as Score).min(20);
        best = Some(best.map_or(penalty, |b| b.min(penalty)));
    }
    best
}

impl Term {
    fn score(&self, h: &Haystack<'_>) -> Option<Score> {
        let t = self.text.as_str();
        let has = |field: &str| field.to_lowercase().contains(t);
        let chars: Vec<char> = t.chars().collect();
        let fuzzy_in =
            |tier: Score, field: &str| fuzzy(&chars, field).map(|p| tier * TIER + p.min(TIER - 1));
        let tag = || h.tags.iter().any(|g| g.to_lowercase().contains(t));

        let title = h.title.to_lowercase();
        match self.scope {
            Scope::Title => {
                if title.starts_with(t) {
                    Some(TITLE_PREFIX * TIER)
                } else if title.contains(t) {
                    Some(TITLE * TIER)
                } else {
                    fuzzy_in(FUZZY_TITLE, h.title)
                }
            }
            Scope::User => {
                if has(h.username) {
                    Some(USER * TIER)
                } else {
                    fuzzy_in(FUZZY_USER, h.username)
                }
            }
            Scope::Url => {
                if has(h.url) {
                    Some(URL * TIER)
                } else {
                    fuzzy_in(FUZZY_URL, h.url)
                }
            }
            Scope::Tag => tag().then_some(TAG * TIER),
            Scope::Any => {
                if title.starts_with(t) {
                    Some(TITLE_PREFIX * TIER)
                } else if title.contains(t) {
                    Some(TITLE * TIER)
                } else if has(h.username) {
                    Some(USER * TIER)
                } else if has(h.url) {
                    Some(URL * TIER)
                } else if tag() {
                    Some(TAG * TIER)
                } else if has(h.notes) {
                    Some(NOTES * TIER)
                } else {
                    fuzzy_in(FUZZY_TITLE, h.title)
                        .or_else(|| fuzzy_in(FUZZY_USER, h.username))
                        .or_else(|| fuzzy_in(FUZZY_URL, h.url))
                }
            }
        }
    }
}

impl Query {
    pub fn parse(query: &str) -> Query {
        let terms = words(query)
            .into_iter()
            .filter_map(|w| {
                let (scope, text) = match w.split_once(':') {
                    Some((p, rest)) => match scope_of(p) {
                        Some(s) => (s, rest.to_string()),
                        // not a prefix we know, e.g. "https://..."
                        None => (Scope::Any, w.clone()),
                    },
                    None => (Scope::Any, w.clone()),
                };
                let text = text.trim().to_lowercase();
                (!text.is_empty()).then_some(Term { scope, text })
            })
            .collect();
        Query { terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// `None` when the entry does not match. An empty query matches
    /// everything equally.
    pub fn rank(&self, h: &Haystack<'_>) -> Option<Score> {
        self.terms
            .iter()
            .try_fold(0, |sum, t| Some(sum + t.score(h)?))
    }
}

/// Ranks one entry against a query string. Convenience for callers that
/// search a single item; parse once with [`Query::parse`] for many.
pub fn rank(query: &str, h: &Haystack<'_>) -> Option<Score> {
    Query::parse(query).rank(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct E {
        title: &'static str,
        username: &'static str,
        url: &'static str,
        notes: &'static str,
        tags: Vec<String>,
    }

    fn e(title: &'static str, username: &'static str, url: &'static str) -> E {
        E {
            title,
            username,
            url,
            notes: "",
            tags: Vec::new(),
        }
    }

    impl E {
        fn notes(mut self, n: &'static str) -> E {
            self.notes = n;
            self
        }
        fn tag(mut self, t: &str) -> E {
            self.tags.push(t.to_string());
            self
        }
        fn rank(&self, q: &str) -> Option<Score> {
            rank(
                q,
                &Haystack {
                    title: self.title,
                    username: self.username,
                    url: self.url,
                    notes: self.notes,
                    tags: &self.tags,
                },
            )
        }
    }

    #[test]
    fn ranks_substring_matches_by_field() {
        let tiers = [
            e("GitHub", "", "").rank("git"),
            e("GitHub", "", "").rank("hub"),
            e("VPN", "lin@acme.cn", "").rank("ACME"),
            e("VPN", "", "https://vpn.acme.cn").rank("acme"),
            e("VPN", "", "").tag("工作").rank("工作"),
            e("银行", "", "").notes("U 盾在抽屉里").rank("抽屉"),
            e("GitHub", "", "").rank("gthb"),
        ];
        let scores: Vec<Score> = tiers.iter().map(|s| s.expect("matches")).collect();
        assert!(scores.windows(2).all(|w| w[0] < w[1]), "{scores:?}");
        assert_eq!(e("GitHub", "", "").rank("zzz"), None);
    }

    #[test]
    fn an_empty_query_matches_everything() {
        assert_eq!(e("anything", "", "").rank(""), Some(0));
        assert_eq!(e("anything", "", "").rank("   "), Some(0));
        assert!(Query::parse("  ").is_empty());
    }

    #[test]
    fn every_word_must_match() {
        let gh = e("GitHub", "linchen", "https://github.com");
        assert!(gh.rank("git lin").is_some());
        assert!(gh.rank("git nobody").is_none());
    }

    #[test]
    fn prefixes_limit_the_field() {
        let gh = e("GitHub", "alice", "https://github.com/login").tag("dev");
        assert!(gh.rank("title:git").is_some());
        assert!(gh.rank("title:alice").is_none());
        assert!(gh.rank("user:ali").is_some());
        assert!(gh.rank("USER:ali").is_some());
        assert!(gh.rank("user:git").is_none());
        assert!(gh.rank("url:login").is_some());
        assert!(gh.rank("url:alice").is_none());
        assert!(gh.rank("tag:dev").is_some());
        assert!(gh.rank("tag:git").is_none());
        // an empty prefix is ignored rather than matching nothing
        assert!(gh.rank("title:").is_some());
        // unknown prefixes are plain text, so URLs can be searched as typed
        assert!(gh.rank("https://github").is_some());
    }

    #[test]
    fn quotes_keep_spaces() {
        let bank = e("My Bank", "", "");
        assert!(bank.rank("\"my bank\"").is_some());
        assert!(bank.rank("title:\"my bank\"").is_some());
        assert!(e("Bank of my town", "", "").rank("\"my bank\"").is_none());
    }

    #[test]
    fn fuzzy_matches_need_letters_close_together() {
        assert!(e("GitHub", "", "").rank("gthb").is_some());
        assert!(e("Bilibili", "", "").rank("bli").is_some());
        // the letters exist but far apart: no match
        assert!(e("abcdefghijklmnopqrstuvwxyz", "", "").rank("az").is_none());
        // one letter only matches as a substring
        assert!(e("GitHub", "", "").rank("q").is_none());
        // a tighter fuzzy match ranks first
        let tight = e("gthub", "", "").rank("gthb").unwrap();
        let loose = e("g-t-h-b", "", "").rank("gthb").unwrap();
        assert!(tight < loose);
    }
}
