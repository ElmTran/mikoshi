import { describe, expect, it } from 'vitest';
import { baseName, host, searchTerms } from './text';

describe('searchTerms', () => {
  it('drops field prefixes and keeps quoted spaces', () => {
    expect(searchTerms('git  hub')).toEqual(['git', 'hub']);
    expect(searchTerms('title:git USER:alice url:bank tag:工作')).toEqual(['git', 'alice', 'bank', '工作']);
    expect(searchTerms('title:"my bank" other')).toEqual(['my bank', 'other']);
    expect(searchTerms('title: https://x.com')).toEqual(['https://x.com']);
    expect(searchTerms('   ')).toEqual([]);
  });
});

describe('host and baseName', () => {
  it('take the interesting part', () => {
    expect(host('https://github.com/login')).toBe('github.com');
    expect(baseName(String.raw`D:\a\b.kdbx`)).toBe('b.kdbx');
    expect(baseName('/home/u/b.kdbx')).toBe('b.kdbx');
  });
});
