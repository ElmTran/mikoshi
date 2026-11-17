//! Ranking for search results. Lower is better; `None` means no match.
//!
//! Case-insensitive substring matching over title, username, URL, notes and
//! tags (features.md §3.4). Ranked: title starts with the query, title
//! contains it, then username, URL, tags, notes.

pub struct Haystack<'a> {
    pub title: &'a str,
    pub username: &'a str,
    pub url: &'a str,
    pub notes: &'a str,
    pub tags: &'a [String],
}

pub fn rank(query: &str, h: &Haystack<'_>) -> Option<u8> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let title = h.title.to_lowercase();
    if title.starts_with(&q) {
        return Some(0);
    }
    if title.contains(&q) {
        return Some(1);
    }
    if h.username.to_lowercase().contains(&q) {
        return Some(2);
    }
    if h.url.to_lowercase().contains(&q) {
        return Some(3);
    }
    if h.tags.iter().any(|t| t.to_lowercase().contains(&q)) {
        return Some(4);
    }
    if h.notes.to_lowercase().contains(&q) {
        return Some(5);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h<'a>(
        title: &'a str,
        username: &'a str,
        url: &'a str,
        notes: &'a str,
        tags: &'a [String],
    ) -> Haystack<'a> {
        Haystack {
            title,
            username,
            url,
            notes,
            tags,
        }
    }

    #[test]
    fn ranks_title_prefix_first() {
        let none: Vec<String> = Vec::new();
        assert_eq!(rank("git", &h("GitHub", "", "", "", &none)), Some(0));
        assert_eq!(rank("hub", &h("GitHub", "", "", "", &none)), Some(1));
        assert_eq!(
            rank("ACME", &h("VPN", "lin@acme.cn", "", "", &none)),
            Some(2)
        );
        assert_eq!(
            rank("acme", &h("VPN", "", "https://vpn.acme.cn", "", &none)),
            Some(3)
        );
        assert_eq!(
            rank("工作", &h("VPN", "", "", "", &["工作".to_string()])),
            Some(4)
        );
        assert_eq!(
            rank("抽屉", &h("银行", "", "", "U 盾在抽屉里", &none)),
            Some(5)
        );
        assert_eq!(rank("zzz", &h("GitHub", "", "", "", &none)), None);
    }
}
