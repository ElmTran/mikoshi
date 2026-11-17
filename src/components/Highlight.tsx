import type { ReactNode } from 'react';

/**
 * Marks every place a search term occurs in `text` (case-insensitive).
 * Fuzzy matches (letters spread out) are not marked.
 */
export function Highlight({ text, terms }: { text: string; terms: string[] }) {
  const lower = text.toLowerCase();
  const needles = terms.map((t) => t.toLowerCase()).filter(Boolean);
  if (!needles.length) return <>{text}</>;
  const parts: ReactNode[] = [];
  let at = 0;
  while (at < text.length) {
    // the earliest next occurrence of any term, the longest on a tie
    let hit = -1;
    let len = 0;
    for (const n of needles) {
      const i = lower.indexOf(n, at);
      if (i >= 0 && (hit < 0 || i < hit || (i === hit && n.length > len))) {
        hit = i;
        len = n.length;
      }
    }
    if (hit < 0) break;
    if (hit > at) parts.push(text.slice(at, hit));
    parts.push(<mark key={hit}>{text.slice(hit, hit + len)}</mark>);
    at = hit + len;
  }
  if (at < text.length) parts.push(text.slice(at));
  return <>{parts}</>;
}
