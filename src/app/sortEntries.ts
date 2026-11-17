import type { EntrySort, EntrySummary } from '../api/types';
import { host } from '../text';

const sortValue = (e: EntrySummary, key: EntrySort['key']) => {
  switch (key) {
    case 'title':
      return e.title;
    case 'username':
      return e.username;
    case 'url':
      // by site, not by "https://"
      return e.url ? `${host(e.url)} ${e.url}` : '';
    case 'modified':
      return e.modified ?? '';
  }
};

/**
 * Sorts a group's entries for the list. Entries without the value (no user
 * name, no URL) go last whichever way the list is sorted; ties go by title.
 */
export function sortEntries(list: EntrySummary[], sort: EntrySort, collator: Intl.Collator): EntrySummary[] {
  const dir = sort.desc ? -1 : 1;
  return [...list].sort((a, b) => {
    const va = sortValue(a, sort.key);
    const vb = sortValue(b, sort.key);
    if (!va !== !vb) return va ? -1 : 1;
    // ISO timestamps sort as plain text
    const c = sort.key === 'modified' ? (va < vb ? -1 : va > vb ? 1 : 0) : collator.compare(va, vb);
    return c * dir || collator.compare(a.title, b.title);
  });
}
