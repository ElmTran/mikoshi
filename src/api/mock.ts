// Browser-only stand-in for the Rust commands, used by `pnpm dev` outside
// Tauri so the UI can be worked on in a normal browser. Demo data, same rules
// (nothing secret in the views). Never part of a production build.

import demoZh from './mock-vault.json';
import demoEn from './mock-vault.en.json';
import { resolveLocale } from '../i18n/core';
import {
  AppError,
  DEFAULT_SETTINGS,
  type EditableEntry,
  type EntryDetail,
  type EntryEdit,
  type EntrySummary,
  type GroupKind,
  type GroupView,
  type HistoryItem,
  type Settings,
  type TotpView,
  type VaultStatus,
} from './types';

interface DemoEntry {
  id: string;
  group: string;
  title: string;
  username: string;
  password: string;
  url: string;
  tags?: string[];
  otp?: boolean;
  created: string;
  modified: string;
  expires?: string | null;
  notes?: string;
  fields?: { key: string; value: string; protected: boolean }[];
  attachments?: { name: string; size: number }[];
  autotype?: { enabled: boolean; sequence: string; associations: { window: string; sequence: string }[] };
}

/** Demo data in the UI language (chosen once, at load), for screenshots. */
function demoLocale() {
  try {
    const saved = JSON.parse(localStorage.getItem('mikoshi.settings') ?? '{}') as Partial<Settings>;
    return resolveLocale(saved.language ?? 'system');
  } catch {
    return resolveLocale('system');
  }
}

const data = (demoLocale() === 'zh-CN' ? demoZh : demoEn) as {
  name: string;
  file: string;
  path: string;
  groups: typeof demoZh.groups;
  entries: DemoEntry[];
};

/** Standard icons for the demo data, so the browser preview is not all keys. */
const ICONS: Record<string, number> = {
  root: 49,
  fav: 61,
  work: 59,
  srv: 3,
  fin: 66,
  social: 5,
  game: 11,
  bin: 43,
  github: 30,
  jd: 47,
  mail163: 19,
  router: 3,
  outlook: 19,
  vpn: 24,
  gitlab: 30,
  jenkins: 33,
  wiki: 65,
  aliyun: 27,
  notion: 44,
  prod: 3,
  staging: 3,
  pg: 42,
  cmb: 66,
  alipay: 37,
  tax: 67,
  bili: 28,
  douban: 56,
  steam: 68,
};

let locked = true;
let current: string | null = data.path;
// unsaved edits, as in the real app
let dirty = false;
// older versions by entry id, oldest first
const past = new Map<string, DemoEntry[]>();
const bus = new EventTarget();

function emit(event: string, payload: unknown) {
  bus.dispatchEvent(new CustomEvent(event, { detail: payload }));
}

export function listen(event: string, handler: (payload: unknown) => void): () => void {
  const fn = (e: Event) => handler((e as CustomEvent).detail);
  bus.addEventListener(event, fn);
  return () => bus.removeEventListener(event, fn);
}

const SETTINGS_KEY = 'mikoshi.settings';

function loadSettings(): Settings {
  try {
    return { ...DEFAULT_SETTINGS, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

function saveSettings(s: Settings) {
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(s));
  emit('settings-changed', s);
  return s;
}

const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;

const status = (): VaultStatus => ({
  locked,
  path: current,
  file: current ? baseName(current) : null,
  name: locked ? (current ? baseName(current).replace(/\.kdbx$/i, '') : '') : data.name,
  keyFile: loadSettings().recent.find((r) => r.path === current)?.keyFile ?? null,
  dirty: !locked && dirty,
});

/** Entries get a new "modified" time on every edit; keep one clock for all. */
const now = () => new Date().toISOString().replace(/\.\d+Z$/, 'Z');

function remember(e: DemoEntry) {
  const list = past.get(e.id) ?? [];
  list.push({ ...e });
  past.set(e.id, list);
}

function applyEdit(e: DemoEntry, edit: EntryEdit) {
  remember(e);
  e.title = edit.title;
  e.username = edit.username;
  e.password = edit.password;
  e.url = edit.url;
  e.notes = edit.notes;
  e.tags = edit.tags;
  e.expires = edit.expires;
  e.fields = edit.fields.map((f) => ({ key: f.key, value: f.value, protected: f.protected }));
  if (edit.autotype)
    e.autotype = {
      ...edit.autotype,
      associations: edit.autotype.associations.filter((x) => x.window.trim()),
    };
  if (edit.totp.kind === 'remove') e.otp = false;
  else if (edit.totp.kind !== 'unchanged') e.otp = true;
  e.modified = now();
}

function editable(e: DemoEntry): EditableEntry {
  return {
    id: e.id,
    groupId: e.group,
    title: e.title,
    username: e.username,
    password: e.password,
    url: e.url,
    notes: e.notes ?? '',
    tags: e.tags ?? [],
    expires: e.expires ?? null,
    fields: (e.fields ?? []).map((f) => ({ ...f })),
    totp: e.otp ? { valid: true, algorithm: 'SHA1', digits: 6, period: 30, steam: false } : null,
    icon: ICONS[e.id] ?? 0,
    customIcon: null,
    autotype: {
      enabled: e.autotype?.enabled ?? true,
      sequence: e.autotype?.sequence ?? '',
      inherited: '{USERNAME}{TAB}{PASSWORD}{ENTER}',
      associations: (e.autotype?.associations ?? []).map((x) => ({ ...x })),
    },
  };
}

/** A rough stand-in for the Rust parser: braces must close and name something known. */
function checkSequence(
  sequence: string,
): { problem: 'unknown'; name: string } | { problem: 'unclosed' } | null {
  const known =
    /^(USERNAME|PASSWORD|URL|TITLE|NOTES|TOTP|TIMEOTP|TAB|ENTER|SPACE|CLEARFIELD|BACKSPACE|BS|DEL|DELETE|HOME|END|UP|DOWN|LEFT|RIGHT|ESC|GROUP|UUID|S:.*|DELAY[ =]d+|TAB d+|[+^%~(){}[]])$/i;
  for (const m of sequence.matchAll(/{([^}]*)}?/g)) {
    if (!m[0].endsWith('}')) return { problem: 'unclosed' };
    if (!known.test(m[1] ?? '')) return { problem: 'unknown', name: m[1] ?? '' };
  }
  return null;
}

function detailOf(e: DemoEntry): EntryDetail {
  const path: string[] = [];
  let g = data.groups.find((x) => x.id === e.group);
  while (g) {
    path.unshift(g.name);
    const parent = g.parent;
    g = parent ? data.groups.find((x) => x.id === parent) : undefined;
  }
  return {
    ...summary(e),
    groupPath: path,
    notes: e.notes ?? '',
    created: e.created,
    expires: e.expires ?? null,
    hasPassword: Boolean(e.password),
    fields: (e.fields ?? []).map((f) => ({
      key: f.key,
      protected: f.protected,
      value: f.protected ? null : f.value,
    })),
    attachments: e.attachments ?? [],
  };
}

function announce() {
  const s = status();
  emit('vault-changed', s);
  return s;
}

const kindOf = (id: string) => data.groups.find((g) => g.id === id)?.kind as GroupKind | undefined;
const inBin = (e: DemoEntry) => kindOf(e.group) === 'recycleBin';
const expired = (e: DemoEntry) => Boolean(e.expires && e.expires <= new Date().toISOString().slice(0, 19));

function open() {
  if (locked) throw new AppError('locked');
}

function entriesIn(groupId: string) {
  return kindOf(groupId) === 'root'
    ? data.entries.filter((e) => !inBin(e))
    : data.entries.filter((e) => e.group === groupId);
}

function summary(e: DemoEntry): EntrySummary {
  return {
    id: e.id,
    groupId: e.group,
    title: e.title,
    username: e.username,
    url: e.url,
    modified: e.modified,
    expired: expired(e),
    hasTotp: Boolean(e.otp),
    tags: e.tags ?? [],
    icon: ICONS[e.id] ?? 0,
    customIcon: null,
  };
}

function rank(q: string, e: DemoEntry): number | null {
  const t = q.trim().toLowerCase();
  if (!t) return 0;
  const title = e.title.toLowerCase();
  if (title.startsWith(t)) return 0;
  if (title.includes(t)) return 1;
  if (e.username.toLowerCase().includes(t)) return 2;
  if (e.url.toLowerCase().includes(t)) return 3;
  if ((e.tags ?? []).some((x) => x.toLowerCase().includes(t))) return 4;
  if ((e.notes ?? '').toLowerCase().includes(t)) return 5;
  return null;
}

function find(id: string): DemoEntry {
  open();
  const e = data.entries.find((x) => x.id === id);
  if (!e) throw new AppError('notFound');
  return e;
}

function depth(id: string): number {
  let d = 0;
  let g = data.groups.find((x) => x.id === id);
  while (g?.parent) {
    d += 1;
    const parent: string = g.parent;
    g = data.groups.find((x) => x.id === parent);
  }
  return d;
}

function totp(e: DemoEntry): TotpView {
  const now = Math.floor(Date.now() / 1000);
  let h = 2166136261;
  for (const ch of `${e.id}${Math.floor(now / 30)}`) h = Math.imul(h ^ ch.charCodeAt(0), 16777619) >>> 0;
  return { code: String(h % 1000000).padStart(6, '0'), period: 30, remaining: 30 - (now % 30) };
}

function field(e: DemoEntry, key: string): string {
  switch (key) {
    case 'Title':
      return e.title;
    case 'UserName':
      return e.username;
    case 'Password':
      return e.password;
    case 'URL':
      return e.url;
    case 'Notes':
      return e.notes ?? '';
    case '@totp':
      return totp(e).code;
    default: {
      const f = e.fields?.find((x) => x.key === key);
      if (!f) throw new AppError('notFound');
      return f.value;
    }
  }
}

let mockAutostart = false;

export async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const a = args as Record<string, string | null | undefined>;
  const result = ((): unknown => {
    switch (cmd) {
      case 'app_info':
        return { version: '0.1.0', demo: true, startHidden: false, quickGlass: false };
      case 'get_settings':
        return loadSettings();
      case 'set_settings': {
        const kept = loadSettings();
        const s = {
          ...(args.settings as Settings),
          recent: kept.recent,
          quickSaveGroup: kept.quickSaveGroup,
        };
        if (!s.rememberKeyFiles) s.recent = s.recent.map((r) => ({ ...r, keyFile: null }));
        return saveSettings(s);
      }
      case 'vault_status':
        return status();
      case 'select_database':
        locked = true;
        dirty = false;
        current = a.path ?? null;
        return announce();
      case 'choose_database':
        // no file dialog in a browser: pretend the demo file was picked
        locked = true;
        current = data.path;
        return announce();
      case 'choose_key_file':
        return 'D:\\同步盘\\个人密码库.keyx';
      case 'unlock': {
        if (!current) throw new AppError('noDatabase');
        if (!a.password && !a.keyFile) throw new AppError('emptyPassword');
        locked = false;
        const s = loadSettings();
        const path = current;
        saveSettings({
          ...s,
          recent: [
            { path, keyFile: s.rememberKeyFiles ? (a.keyFile ?? null) : null },
            ...s.recent.filter((r) => r.path !== path),
          ].slice(0, 10),
        });
        return announce();
      }
      case 'lock':
        if (dirty && args.discard !== true && loadSettings().lockUnsaved === 'ask')
          throw new AppError('unsavedChanges');
        locked = true;
        dirty = false;
        return announce();
      case 'forget_recent': {
        const s = loadSettings();
        const next = saveSettings({ ...s, recent: s.recent.filter((r) => r.path !== a.path) });
        if (locked && current === a.path) {
          current = next.recent[0]?.path ?? null;
          announce();
        }
        return next;
      }
      case 'list_groups':
        open();
        return data.groups.map((g): GroupView => ({
          id: g.id,
          name: g.name,
          parentId: g.parent,
          depth: depth(g.id),
          kind: g.kind as GroupKind,
          entryCount: entriesIn(g.id).length,
          icon: ICONS[g.id] ?? 48,
          customIcon: null,
          expanded: true,
          hasChildren: data.groups.some((c) => c.parent === g.id),
        }));
      case 'list_entries':
        open();
        return entriesIn(a.groupId ?? '').map(summary);
      case 'search_entries': {
        open();
        const q = a.query ?? '';
        return data.entries
          .filter((e) => !inBin(e))
          .map((e) => [rank(q, e), e] as const)
          .filter((p): p is readonly [number, DemoEntry] => p[0] !== null)
          .sort((x, y) => x[0] - y[0] || y[1].modified.localeCompare(x[1].modified))
          .map(([, e]) => summary(e));
      }
      case 'get_entry':
        return detailOf(find(a.id ?? ''));
      case 'get_entry_for_edit':
        return editable(find(a.entryId ?? ''));
      case 'check_autotype_sequence':
        return checkSequence(String(args.sequence ?? ''));
      case 'create_entry': {
        open();
        const edit = args.edit as EntryEdit;
        const entry: DemoEntry = {
          id: `new-${Date.now()}`,
          group: a.groupId ?? '',
          title: '',
          username: '',
          password: '',
          url: '',
          created: now(),
          modified: now(),
        };
        applyEdit(entry, edit);
        past.delete(entry.id);
        data.entries.push(entry);
        dirty = true;
        emit('vault-edited', status());
        return entry.id;
      }
      case 'save_entry': {
        const e = find(a.entryId ?? '');
        const before = JSON.stringify(e);
        const edit = args.edit as EntryEdit;
        remember(e);
        applyEdit(e, edit);
        if (JSON.stringify(e) === before) {
          past.get(e.id)?.pop();
          return false;
        }
        dirty = true;
        emit('vault-edited', status());
        return true;
      }
      case 'list_history': {
        const e = find(a.entryId ?? '');
        return (past.get(e.id) ?? []).map((h, index): HistoryItem => ({
          index,
          modified: h.modified,
          title: h.title,
          username: h.username,
        }));
      }
      case 'get_history_entry': {
        const e = find(a.entryId ?? '');
        const h = past.get(e.id)?.[Number(a.index ?? -1)];
        if (!h) throw new AppError('notFound');
        return detailOf(h);
      }
      case 'reveal_history_field': {
        const e = find(a.entryId ?? '');
        const h = past.get(e.id)?.[Number(a.index ?? -1)];
        if (!h) throw new AppError('notFound');
        return field(h, a.key ?? '');
      }
      case 'restore_history': {
        const e = find(a.entryId ?? '');
        const h = past.get(e.id)?.[Number(a.index ?? -1)];
        if (!h) throw new AppError('notFound');
        remember(e);
        Object.assign(e, { ...h, id: e.id, group: e.group, modified: now() });
        dirty = true;
        emit('vault-edited', announce());
        return status();
      }
      case 'save_database':
        open();
        dirty = false;
        return announce();
      case 'reload_database':
        open();
        dirty = false;
        return announce();
      case 'quit_app':
        return null;
      case 'get_custom_icons':
        open();
        return [];
      case 'reveal_field':
        return field(find(a.entryId ?? ''), a.key ?? '');
      case 'copy_field': {
        const e = find(a.entryId ?? '');
        const key = a.key ?? '';
        if (key === '@totp' && !e.otp) throw new AppError('notFound');
        const value = field(e, key);
        if (!value) throw new AppError('notFound');
        // a browser page may write the clipboard only on a user gesture;
        // good enough for UI work
        void navigator.clipboard?.writeText(value).catch(() => {});
        const secret =
          key === 'Password' || key === '@totp' || Boolean(e.fields?.find((f) => f.key === key)?.protected);
        const seconds = loadSettings().clipboardClearSeconds;
        return { clearsAfter: secret && seconds > 0 ? seconds : null };
      }
      case 'open_url': {
        const e = find(a.entryId ?? '');
        if (!/^https?:\/\//i.test(e.url)) throw new AppError('unsafeUrl');
        window.open(e.url, '_blank', 'noopener');
        return null;
      }
      case 'get_totp': {
        const e = find(a.entryId ?? '');
        if (!e.otp) throw new AppError('notFound');
        return totp(e);
      }
      case 'show_quick_search':
        window.open('/?window=quick', 'mikoshi-quick', 'width=700,height=540');
        return null;
      case 'hide_quick_search':
        return null;
      case 'hotkey_status':
      case 'pause_hotkeys': {
        const s = loadSettings();
        const state = (k: string) => (k ? 'active' : 'off');
        return {
          quickSearch: state(s.quickSearchHotkey),
          lock: state(s.lockHotkey),
          autotype: state(s.autotypeHotkey),
          quickSave: state(s.quickSaveHotkey),
        };
      }
      case 'quick_save_prefill': {
        open();
        // a browser page stands in for the window behind the box
        const url = 'https://github.com';
        const groups = data.groups
          .filter((g) => g.kind !== 'recycleBin')
          .map((g) => ({ id: g.id, name: g.name, depth: depth(g.id) }));
        const last = loadSettings().quickSaveGroup;
        return {
          title: 'github.com',
          url,
          username: '',
          groups,
          groupId: groups.some((g) => g.id === last) ? last : groups[0]?.id,
          existing: data.entries.filter((e) => !inBin(e) && /(^|[/.])github\.com/i.test(e.url)).map(summary),
          canType: false,
        };
      }
      case 'quick_save': {
        open();
        const entry = args.entry as {
          groupId: string;
          title: string;
          username: string;
          password: string;
          url: string;
        };
        data.entries.push({
          id: `new-${Date.now()}`,
          group: entry.groupId,
          title: entry.title.trim(),
          username: entry.username.trim(),
          password: entry.password,
          url: entry.url.trim(),
          created: now(),
          modified: now(),
        });
        const s = loadSettings();
        if (s.quickSaveGroup !== entry.groupId) saveSettings({ ...s, quickSaveGroup: entry.groupId });
        emit('vault-edited', status());
        const file = s.saveOnEdit ? 'written' : 'pending';
        dirty = !s.saveOnEdit;
        announce();
        return { file, typeError: args.typePassword ? { code: 'autotypeNoWindow' } : null };
      }
      case 'autotype_candidates':
        return [];
      case 'autotype_entry':
      case 'autotype_from_main':
        // no other windows in a browser
        throw new AppError('autotypeNoWindow');
      case 'get_autostart':
        return mockAutostart;
      case 'set_autostart':
        mockAutostart = Boolean(args.enabled);
        return mockAutostart;
      default:
        throw new AppError('internal');
    }
  })();
  return result as T;
}
