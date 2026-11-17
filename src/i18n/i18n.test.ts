import { describe, expect, it } from 'vitest';
import { catalogs, resolveLocale, translate, translatePlural } from './core';

describe('catalogs', () => {
  it('have the same keys and placeholders in every language', () => {
    const zh = catalogs['zh-CN'];
    const en = catalogs['en-US'];
    expect(Object.keys(en).sort()).toEqual(Object.keys(zh).sort());
    const vars = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort();
    for (const key of Object.keys(zh) as (keyof typeof zh)[]) {
      expect(vars(en[key]), key).toEqual(vars(zh[key]));
      expect(en[key].trim(), key).not.toBe('');
    }
  });
});

describe('translate', () => {
  it('fills in variables', () => {
    expect(translate('en-US', 'search.results', { q: 'git' })).toBe('Results for “git”');
    expect(translate('zh-CN', 'search.results', { q: 'git' })).toBe('搜索“git”');
  });

  it('picks plural forms', () => {
    expect(translatePlural('en-US', 'list.count', 1)).toBe('1 entry');
    expect(translatePlural('en-US', 'list.count', 3)).toBe('3 entries');
    expect(translatePlural('zh-CN', 'list.count', 1)).toBe('1 个条目');
  });

  it('follows the system language unless one is chosen', () => {
    expect(resolveLocale('system', 'zh-CN')).toBe('zh-CN');
    expect(resolveLocale('system', 'zh-TW')).toBe('zh-CN');
    expect(resolveLocale('system', 'de-DE')).toBe('en-US');
    expect(resolveLocale('en-US', 'zh-CN')).toBe('en-US');
  });
});
