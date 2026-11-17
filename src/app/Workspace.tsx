import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../api';
import {
  AppError,
  Field,
  type EntryDetail as Detail,
  type EntrySummary,
  type GroupView,
  type VaultStatus,
} from '../api/types';
import { EntryDetail, type DetailActions } from '../components/EntryDetail';
import { EntryList } from '../components/EntryList';
import { GroupNav } from '../components/GroupNav';
import { Icon } from '../components/Icon';
import { Keys } from '../components/bits';
import { TitleBar } from '../components/TitleBar';
import { usePrefs, useToast } from './contexts';

const host = (url: string) => url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '').split('/')[0] ?? url;

/** The unlocked main window: groups, entries, details, and all shortcuts. */
export function Workspace({ status }: { status: VaultStatus }) {
  const { t } = usePrefs();
  const toast = useToast();
  const [groups, setGroups] = useState<GroupView[]>([]);
  const [groupId, setGroupId] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [entries, setEntries] = useState<EntrySummary[]>([]);
  const [selectedId, setSelectedIdRaw] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<Detail | null>(null);
  // values the user chose to show, tied to the entry they belong to
  const [shown, setShown] = useState<{ id: string | null; values: Record<string, string> }>({
    id: null,
    values: {},
  });
  const [copied, setCopied] = useState<string | null>(null);
  const search = useRef<HTMLInputElement>(null);
  const copiedTimer = useRef(0);
  const selectedRef = useRef<string | null>(null);

  const detail = loaded && loaded.id === selectedId ? loaded : null;
  const revealed = useMemo(() => (detail && shown.id === detail.id ? shown.values : {}), [detail, shown]);

  /** Selecting another entry drops every revealed value right away. */
  const setSelectedId = useCallback((id: string | null) => {
    selectedRef.current = id;
    setSelectedIdRaw(id);
    setShown((s) => (s.id === id ? s : { id: null, values: {} }));
  }, []);

  const fail = useCallback(
    (e: unknown) => {
      const code = e instanceof AppError ? e.code : 'internal';
      // a locked vault switches the window to the unlock screen via the event
      if (code !== 'locked') toast({ message: t(`error.${code}`), tone: 'error' });
    },
    [t, toast],
  );

  useEffect(() => {
    api.listGroups().then((gs) => {
      setGroups(gs);
      setGroupId((cur) => cur ?? gs.find((g) => g.kind === 'root')?.id ?? gs[0]?.id ?? null);
    }, fail);
  }, [fail]);

  const q = query.trim();
  useEffect(() => {
    if (!q && !groupId) return;
    let live = true;
    (q ? api.searchEntries(q) : api.listEntries(groupId ?? '')).then((list) => {
      if (!live) return;
      setEntries(list);
      const cur = selectedRef.current;
      setSelectedId(cur && list.some((e) => e.id === cur) ? cur : (list[0]?.id ?? null));
    }, fail);
    return () => {
      live = false;
    };
  }, [q, groupId, fail, setSelectedId]);

  useEffect(() => {
    if (!selectedId) return;
    let live = true;
    api.getEntry(selectedId).then((d) => live && setLoaded(d), fail);
    return () => {
      live = false;
    };
  }, [selectedId, fail]);

  const actions: DetailActions = useMemo(() => {
    const label = (key: string) =>
      key === Field.UserName
        ? t('field.username')
        : key === Field.Password
          ? t('field.password')
          : key === Field.URL
            ? t('field.url')
            : key === Field.Totp
              ? t('field.totp')
              : t('toast.quoted', { name: key });
    return {
      copy: (key) => {
        if (!detail) return;
        api.copyField(detail.id, key).then((r) => {
          window.clearTimeout(copiedTimer.current);
          setCopied(key);
          copiedTimer.current = window.setTimeout(() => setCopied(null), 1400);
          toast(
            r.clearsAfter
              ? {
                  message: t('toast.copiedClears', { field: label(key), n: r.clearsAfter }),
                  clearsAfter: r.clearsAfter,
                }
              : { message: t('toast.copied', { field: label(key) }) },
          );
        }, fail);
      },
      toggleReveal: (key) => {
        if (!detail) return;
        const id = detail.id;
        if (key in revealed) {
          setShown((s) => {
            const values = { ...s.values };
            delete values[key];
            return { id, values };
          });
          return;
        }
        api.revealField(id, key).then((v) => {
          // ignore the answer if the user has moved on to another entry
          if (selectedRef.current === id) {
            setShown((s) => ({ id, values: { ...(s.id === id ? s.values : {}), [key]: v } }));
          }
        }, fail);
      },
      openUrl: () =>
        detail && toast({ message: t('toast.openUrl', { host: host(detail.url) }), tone: 'info' }),
      autotype: () => toast({ message: t('toast.autotype'), tone: 'info' }),
      edit: () => toast({ message: t('toast.edit'), tone: 'info' }),
      exportAttachment: () => toast({ message: t('toast.export'), tone: 'info' }),
    };
  }, [detail, revealed, t, toast, fail]);

  const lock = useCallback(() => {
    setShown({ id: null, values: {} });
    api.lock().catch(fail);
  }, [fail]);

  const move = useCallback(
    (dir: 1 | -1) => {
      const i = entries.findIndex((e) => e.id === selectedId);
      const next = entries[Math.max(0, Math.min(entries.length - 1, i + dir))];
      if (next) setSelectedId(next.id);
    },
    [entries, selectedId, setSelectedId],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      const ctrl = e.ctrlKey || e.metaKey;
      const target = e.target as HTMLElement | null;
      const typing = target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA';
      const run = (fn: () => void) => {
        e.preventDefault();
        fn();
      };

      if (ctrl && e.altKey && k === 'k') return run(() => void api.showQuickSearch().catch(fail));
      if (e.altKey) return;
      if (ctrl && !e.shiftKey) {
        if (k === 'f') return run(() => search.current?.select());
        if (k === 'l') return run(lock);
        if (k === 'b') return run(() => actions.copy(Field.UserName));
        if (k === 'u') return run(() => actions.copy(Field.URL));
        if (k === 't') return run(() => detail?.hasTotp && actions.copy(Field.Totp));
        if (k === 'h') return run(() => detail?.hasPassword && actions.toggleReveal(Field.Password));
        if (k === 'e') return run(actions.edit);
        if (k === 'n') return run(() => toast({ message: t('toast.newEntry'), tone: 'info' }));
        if (k === ',')
          return run(() => document.querySelector<HTMLButtonElement>('[data-settings-toggle]')?.click());
        if (k === 'c') {
          // inside a text box with a selection, Ctrl+C copies the selection
          const el = target as HTMLInputElement | null;
          if (typing && el && el.selectionStart !== el.selectionEnd) return;
          return run(() => detail?.hasPassword && actions.copy(Field.Password));
        }
      }
      if (ctrl && e.shiftKey && k === 'u') return run(actions.openUrl);
      if (ctrl && e.shiftKey && k === 'v') return run(actions.autotype);
      if (k === 'ArrowDown' || k === 'ArrowUp') return run(() => move(k === 'ArrowDown' ? 1 : -1));
      if (k === 'Escape' && typing && query) return run(() => setQuery(''));
      if (k === 'Enter' && !typing && detail) return run(actions.edit);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [actions, detail, fail, lock, move, query, t, toast]);

  const listTitle = q ? t('search.results', { q }) : (groups.find((g) => g.id === groupId)?.name ?? '');

  const searchBox = (
    <label className="search">
      <Icon name="search" />
      <input
        ref={search}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={t('search.placeholder')}
        aria-label={t('search.label')}
        spellCheck={false}
        autoComplete="off"
      />
      <Keys keys={['Ctrl', 'F']} />
    </label>
  );

  return (
    <div className="shell workspace">
      <TitleBar
        fileName={status.file}
        center={searchBox}
        onQuickSearch={() => void api.showQuickSearch().catch(fail)}
        onLock={lock}
      />
      <div className="body">
        <GroupNav
          groups={groups}
          selected={q ? null : groupId}
          onSelect={(id) => {
            setQuery('');
            setGroupId(id);
          }}
        />
        <div className="content-layer">
          <EntryList
            title={listTitle}
            entries={entries}
            selected={selectedId}
            query={q}
            onSelect={setSelectedId}
            onNew={() => toast({ message: t('toast.newEntry'), tone: 'info' })}
          />
          <section className="detail" aria-live="polite">
            {detail ? (
              <EntryDetail entry={detail} revealed={revealed} copied={copied} actions={actions} />
            ) : (
              <div className="empty">{t('detail.empty')}</div>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
