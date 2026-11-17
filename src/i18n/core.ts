import type { LanguagePref } from '../api/types';
import { enUS } from './en-US';
import { zhCN, type MessageKey } from './zh-CN';

export type Locale = 'zh-CN' | 'en-US';
export type { MessageKey };

export const catalogs: Record<Locale, Record<MessageKey, string>> = { 'zh-CN': zhCN, 'en-US': enUS };

export function resolveLocale(pref: LanguagePref, systemLanguage = navigator.language): Locale {
  if (pref !== 'system') return pref;
  return systemLanguage.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US';
}

export type Vars = Record<string, string | number>;

export function translate(locale: Locale, key: MessageKey, vars?: Vars): string {
  const text = catalogs[locale][key];
  return vars ? text.replace(/\{(\w+)\}/g, (m, name: string) => String(vars[name] ?? m)) : text;
}

type PluralBase<K> = K extends `${infer B}.other` ? B : never;
export type PluralKey = PluralBase<MessageKey>;

export function translatePlural(locale: Locale, key: PluralKey, n: number, vars?: Vars): string {
  const form = new Intl.PluralRules(locale).select(n) === 'one' ? 'one' : 'other';
  return translate(locale, `${key}.${form}` as MessageKey, { n, ...vars });
}

export interface Formatters {
  /** null (a time the file does not have) gives a dash */
  date: (iso: string | null) => string;
  dateTime: (iso: string | null) => string;
  size: (bytes: number) => string;
}

export function formatters(locale: Locale): Formatters {
  const date = new Intl.DateTimeFormat(locale, { year: 'numeric', month: '2-digit', day: '2-digit' });
  const dateTime = new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' });
  const num = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });
  // an invalid date would make Intl throw; show a dash instead
  const safe = (f: Intl.DateTimeFormat) => (iso: string | null) => {
    const d = iso ? new Date(iso) : null;
    return d && !Number.isNaN(d.getTime()) ? f.format(d) : '—';
  };
  return {
    date: safe(date),
    dateTime: safe(dateTime),
    size: (b) =>
      b < 1024 ? `${b} B` : b < 1024 * 1024 ? `${num.format(b / 1024)} KB` : `${num.format(b / 1048576)} MB`,
  };
}
