// Browser-only stand-in for the Rust commands, used by `pnpm dev` outside
// Tauri so the UI can be worked on in a normal browser. Demo data, same rules
// (nothing secret in the views). Never part of a production build.

import demo from './mock-vault.json';
import {
  AppError,
  DEFAULT_SETTINGS,
  type EntryDetail,
  type EntrySummary,
  type GroupKind,
  type GroupView,
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
}

const data = demo as {
  name: string;
  file: string;
  path: string;
  groups: typeof demo.groups;
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
});

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

export async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const a = args as Record<string, string | null | undefined>;
  const result = ((): unknown => {
    switch (cmd) {
      case 'app_info':
        return { version: '0.1.0', effects: false, demo: true };
      case 'get_settings':
        return loadSettings();
      case 'set_settings': {
        const s = { ...(args.settings as Settings), recent: loadSettings().recent };
        if (!s.rememberKeyFiles) s.recent = s.recent.map((r) => ({ ...r, keyFile: null }));
        return saveSettings(s);
      }
      case 'vault_status':
        return status();
      case 'select_database':
        locked = true;
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
        locked = true;
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
      case 'get_entry': {
        const e = find(a.id ?? '');
        const path: string[] = [];
        let g = data.groups.find((x) => x.id === e.group);
        while (g) {
          path.unshift(g.name);
          const parent = g.parent;
          g = parent ? data.groups.find((x) => x.id === parent) : undefined;
        }
        const detail: EntryDetail = {
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
        return detail;
      }
      case 'get_custom_icons':
        open();
        return [];
      case 'reveal_field':
        return field(find(a.entryId ?? ''), a.key ?? '');
      case 'copy_field': {
        const e = find(a.entryId ?? '');
        field(e, a.key ?? '');
        // like the app until M3: copying does not reach the clipboard yet
        throw new AppError('notYet');
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
      default:
        throw new AppError('internal');
    }
  })();
  return result as T;
}
