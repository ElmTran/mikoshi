// Small text helpers shared by the windows.

/** The host part of a URL: `https://github.com/login` → `github.com`. */
export const host = (url: string) => url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '').split('/')[0] ?? url;

/** The last part of a Windows or POSIX path. */
export const baseName = (path: string) => path.split(/[/\\]/).pop() ?? path;

const PREFIXES = ['title', 'user', 'username', 'url', 'tag', 'tags'];

/**
 * The words of a search query as typed, for highlighting matches: quotes
 * keep spaces, field prefixes (`title:` ...) are dropped. Mirrors the Rust
 * query parser (crates/mikoshi-core/src/search.rs).
 */
export function searchTerms(query: string): string[] {
  const words: string[] = [];
  let cur = '';
  let quoted = false;
  for (const c of query) {
    if (c === '"') quoted = !quoted;
    else if (/\s/.test(c) && !quoted) {
      if (cur) words.push(cur);
      cur = '';
    } else cur += c;
  }
  if (cur) words.push(cur);
  return words
    .map((w) => {
      const i = w.indexOf(':');
      return i > 0 && PREFIXES.includes(w.slice(0, i).toLowerCase()) ? w.slice(i + 1) : w;
    })
    .map((w) => w.trim())
    .filter(Boolean);
}
