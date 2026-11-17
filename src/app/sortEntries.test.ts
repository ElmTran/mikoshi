import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../api/types';
import { sortEntries } from './sortEntries';

const entry = (title: string, username: string, url: string, modified: string | null): EntrySummary => ({
  id: title,
  groupId: 'g',
  title,
  username,
  url,
  modified,
  expired: false,
  hasTotp: false,
  tags: [],
  icon: 0,
  customIcon: null,
});

const list = [
  entry('b', 'zed', 'https://b.example', '2026-01-02T00:00:00Z'),
  entry('A', '', 'http://a.example', '2026-03-01T00:00:00Z'),
  entry('c10', 'amy', '', null),
  entry('c9', 'amy', 'https://c.example', '2025-12-31T00:00:00Z'),
];
const collator = new Intl.Collator('en-US', { numeric: true, sensitivity: 'base' });
const titles = (l: EntrySummary[]) => l.map((e) => e.title);

describe('sortEntries', () => {
  it('sorts titles naturally and case-insensitively', () => {
    expect(titles(sortEntries(list, { key: 'title', desc: false }, collator))).toEqual([
      'A',
      'b',
      'c9',
      'c10',
    ]);
    expect(titles(sortEntries(list, { key: 'title', desc: true }, collator))).toEqual([
      'c10',
      'c9',
      'b',
      'A',
    ]);
  });

  it('puts missing values last in both directions', () => {
    expect(titles(sortEntries(list, { key: 'username', desc: false }, collator))).toEqual([
      'c9',
      'c10',
      'b',
      'A',
    ]);
    expect(titles(sortEntries(list, { key: 'username', desc: true }, collator))).toEqual([
      'b',
      'c9',
      'c10',
      'A',
    ]);
    expect(titles(sortEntries(list, { key: 'modified', desc: true }, collator))).toEqual([
      'A',
      'b',
      'c9',
      'c10',
    ]);
  });

  it('sorts URLs by site, not by scheme', () => {
    expect(titles(sortEntries(list, { key: 'url', desc: false }, collator))).toEqual(['A', 'b', 'c9', 'c10']);
  });

  it('does not change the input', () => {
    const copy = [...list];
    sortEntries(list, { key: 'title', desc: false }, collator);
    expect(list).toEqual(copy);
  });
});
