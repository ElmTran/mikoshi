import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react';
import { api, listen } from '../api';
import {
  AppError,
  Field,
  type EditableEntry,
  type EntryDetail as Detail,
  type EntryEdit,
  type EntrySort,
  type EntrySummary,
  type GroupView,
  type VaultStatus,
} from '../api/types';
import { Dialog } from '../components/Dialog';
import { EntryDetail, type DetailActions } from '../components/EntryDetail';
import { EntryEditor } from '../components/EntryEditor';
import { EntryList } from '../components/EntryList';
import { GroupNav } from '../components/GroupNav';
import { HistoryDialog } from '../components/HistoryDialog';
import { Icon } from '../components/Icon';
import { Keys } from '../components/bits';
import { TitleBar } from '../components/TitleBar';
import { usePrefs, useToast } from './contexts';
import { sortEntries } from './sortEntries';

type Pane = 'groups' | 'list' | 'detail';

/** What the right pane shows instead of the entry detail. */
type Mode =
  | { kind: 'view' }
  /** the entry fetched with all its values; dropped when editing ends */
  | { kind: 'edit'; entry: EditableEntry }
  | { kind: 'new' };

/** A question the user has to answer before the action goes ahead. */
type Ask =
  | { kind: 'lock' }
  | { kind: 'quit' }
  /** the file was changed by another program */
  | { kind: 'conflict' }
  /** saving turns a KDBX 3.1 or 4.0 file into 4.1 */
  | { kind: 'upgrade' };
const PANES: Pane[] = ['groups', 'list', 'detail'];
const PAGE = 8;

/** Groups the file remembers as collapsed (only those that have children). */
const initiallyCollapsed = (groups: GroupView[]) =>
  new Set(groups.filter((g) => g.hasChildren && !g.expanded).map((g) => g.id));

/** The unlocked main window: group tree, entry list, details, and all shortcuts. */
export function Workspace({ status }: { status: VaultStatus }) {
  const { t, locale, settings, update } = usePrefs();
  const toast = useToast();
  const [groups, setGroups] = useState<GroupView[]>([]);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [groupId, setGroupId] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [entries, setEntries] = useState<EntrySummary[]>([]);
  const [picked, setPicked] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<Detail | null>(null);
  // values the user chose to show, tied to the entry they belong to
  const [shown, setShown] = useState<{ id: string | null; values: Record<string, string> }>({
    id: null,
    values: {},
  });
  const [copied, setCopied] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode>({ kind: 'view' });
  const [ask, setAsk] = useState<Ask | null>(null);
  const [busy, setBusy] = useState(false);
  const [historyFor, setHistoryFor] = useState<string | null>(null);
  // bumped after an edit so the list and the detail load again
  const [version, setVersion] = useState(0);
  // what to do once a save that needed a dialog went through
  const afterSave = useRef<'lock' | 'quit' | null>(null);
  const search = useRef<HTMLInputElement>(null);
  const treeRef = useRef<HTMLElement | null>(null);
  const listRef = useRef<HTMLElement | null>(null);
  const detailRef = useRef<HTMLElement | null>(null);
  const copiedTimer = useRef(0);
  const selectedRef = useRef<string | null>(null);

  const q = query.trim();
  const collator = useMemo(() => new Intl.Collator(locale, { numeric: true, sensitivity: 'base' }), [locale]);
  // search results keep their best-match order; a group's list follows the chosen sort
  const list = useMemo(
    () => (q ? entries : sortEntries(entries, settings.entrySort, collator)),
    [q, entries, settings.entrySort, collator],
  );
  // the picked entry; the first one until the list has loaded
  const selected = picked && list.some((e) => e.id === picked) ? picked : (list[0]?.id ?? null);
  const detail = loaded && loaded.id === selected ? loaded : null;
  // selecting another entry drops every revealed value right away
  const revealed = useMemo(() => (detail && shown.id === detail.id ? shown.values : {}), [detail, shown]);

  useEffect(() => {
    selectedRef.current = selected;
  }, [selected]);

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
      setCollapsed(initiallyCollapsed(gs));
      setGroupId((cur) => cur ?? gs.find((g) => g.kind === 'root')?.id ?? gs[0]?.id ?? null);
    }, fail);
  }, [fail]);

  /**
   * A new list keeps the selected entry if it is in it. Otherwise the first
   * entry (in the order shown) becomes the selection, so it stays put when
   * the list changes again, e.g. when a search is cleared.
   */
  const onEntries = useEffectEvent((l: EntrySummary[], searching: boolean) => {
    setEntries(l);
    const shownOrder = searching ? l : sortEntries(l, settings.entrySort, collator);
    setPicked((cur) => (cur && l.some((e) => e.id === cur) ? cur : (shownOrder[0]?.id ?? null)));
  });

  useEffect(() => {
    if (!q && !groupId) return;
    let live = true;
    (q ? api.searchEntries(q) : api.listEntries(groupId ?? '')).then(
      (l) => live && onEntries(l, Boolean(q)),
      fail,
    );
    return () => {
      live = false;
    };
  }, [q, groupId, fail, version]);

  useEffect(() => {
    if (!selected) return;
    let live = true;
    api.getEntry(selected).then((d) => live && setLoaded(d), fail);
    return () => {
      live = false;
    };
  }, [selected, fail, version]);

  // arrow keys work right away
  useEffect(() => listRef.current?.focus({ preventScroll: true }), []);

  // ------------------------------------------------------------ group tree

  const byId = useMemo(() => new Map(groups.map((g) => [g.id, g])), [groups]);
  const visible = useMemo(() => {
    const hidden = new Set<string>();
    return groups.filter((g) => {
      if (g.parentId && (hidden.has(g.parentId) || collapsed.has(g.parentId))) {
        hidden.add(g.id);
        return false;
      }
      return true;
    });
  }, [groups, collapsed]);

  const isInside = useCallback(
    (id: string | null, ancestor: string) => {
      for (let g = id ? byId.get(id) : undefined; g; g = g.parentId ? byId.get(g.parentId) : undefined) {
        if (g.parentId === ancestor) return true;
      }
      return false;
    },
    [byId],
  );

  const selectGroup = useCallback((id: string) => {
    setQuery('');
    setGroupId(id);
  }, []);

  const setOpen = useCallback(
    (id: string, open: boolean) => {
      setCollapsed((cur) => {
        if (open === !cur.has(id)) return cur;
        const next = new Set(cur);
        if (open) next.delete(id);
        else next.add(id);
        return next;
      });
      // collapsing hides the selected group: select the one being collapsed
      if (!open && isInside(groupId, id)) selectGroup(id);
    },
    [groupId, isInside, selectGroup],
  );

  const toggle = useCallback((id: string) => setOpen(id, collapsed.has(id)), [collapsed, setOpen]);

  // ------------------------------------------------------------ actions

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
      openUrl: () => {
        if (detail?.url) api.openUrl(detail.id).catch(fail);
      },
      autotype: () => {
        if (detail) api.autotypeFromMain(detail.id).catch(fail);
      },
      edit: () => startEditRef.current(),
      history: () => detail && setHistoryFor(detail.id),
      exportAttachment: () => toast({ message: t('toast.export'), tone: 'info' }),
    };
  }, [detail, revealed, t, toast, fail]);

  // ------------------------------------------------------------ editing and saving

  /** The group a new entry goes into: the open one, or the root while searching. */
  const targetGroup = useMemo(() => {
    if (!q && groupId) return byId.get(groupId) ?? null;
    return groups.find((g) => g.kind === 'root') ?? null;
  }, [q, groupId, byId, groups]);

  const startEdit = useCallback(() => {
    if (!detail) return;
    api.getEntryForEdit(detail.id).then((entry) => setMode({ kind: 'edit', entry }), fail);
  }, [detail, fail]);
  // the detail actions are created before this; they call through a ref
  const startEditRef = useRef(startEdit);
  useEffect(() => {
    startEditRef.current = startEdit;
  }, [startEdit]);

  const startNew = useCallback(() => setMode({ kind: 'new' }), []);

  const closeEditor = useCallback(() => {
    setMode({ kind: 'view' });
    setBusy(false);
  }, []);

  const submitEdit = useCallback(
    (edit: EntryEdit) => {
      setBusy(true);
      const done = (id: string) => {
        closeEditor();
        setVersion((v) => v + 1);
        setPicked(id);
        setQuery('');
      };
      const failed = (e: unknown) => {
        setBusy(false);
        // a locked vault switches the window to the unlock screen via the event
        if (!(e instanceof AppError) || e.code !== 'locked') fail(e);
      };
      if (mode.kind === 'edit') {
        api.saveEntry(mode.entry.id, edit).then((changed) => {
          if (changed) done(mode.entry.id);
          else closeEditor();
        }, failed);
      } else if (targetGroup) {
        api.createEntry(targetGroup.id, edit).then(done, failed);
      } else {
        setBusy(false);
      }
    },
    [closeEditor, fail, mode, targetGroup],
  );

  /** After a save that needed a dialog: finish the lock or quit that asked for it. */
  const runAfterSave = useCallback(() => {
    const pending = afterSave.current;
    afterSave.current = null;
    if (pending === 'lock') api.lock(true).catch(fail);
    else if (pending === 'quit') api.quitApp(true).catch(fail);
  }, [fail]);

  const runSave = useCallback(
    (options?: { force?: boolean; upgrade?: boolean }) => {
      // nothing to write: do not touch the file (and do not ask about 4.1)
      if (!options?.force && !options?.upgrade && !status.dirty) return;
      setBusy(true);
      api.saveDatabase(options).then(
        () => {
          setBusy(false);
          setAsk(null);
          toast({ message: t('toast.saved') });
          runAfterSave();
        },
        (e) => {
          setBusy(false);
          const code = e instanceof AppError ? e.code : 'internal';
          if (code === 'saveConflict') setAsk({ kind: 'conflict' });
          else if (code === 'formatUpgrade') setAsk({ kind: 'upgrade' });
          else {
            setAsk(null);
            afterSave.current = null;
            fail(e);
          }
        },
      );
    },
    [fail, runAfterSave, status.dirty, t, toast],
  );

  const lock = useCallback(() => {
    setShown({ id: null, values: {} });
    api.lock().then(
      () => setMode({ kind: 'view' }),
      (e) => {
        if (e instanceof AppError && e.code === 'unsavedChanges') setAsk({ kind: 'lock' });
        else fail(e);
      },
    );
  }, [fail]);

  const quit = useCallback(
    (discard: boolean) =>
      api.quitApp(discard).catch((e: unknown) => {
        if (e instanceof AppError && e.code === 'unsavedChanges') setAsk({ kind: 'quit' });
        else fail(e);
      }),
    [fail],
  );

  // the tray asked to quit while edits are unsaved
  useEffect(() => {
    const off = listen('quit-requested', () => setAsk({ kind: 'quit' }));
    return () => void off.then((f) => f());
  }, []);

  // edits that could not be saved over the file went to a separate one
  useEffect(() => {
    const off = listen<string>('save-conflict-copy', (path) =>
      toast({ message: t('toast.savedCopy', { path }), tone: 'error', seconds: 12 }),
    );
    return () => void off.then((f) => f());
  }, [t, toast]);

  const sortBy = useCallback((entrySort: EntrySort) => void update({ entrySort }), [update]);

  // ------------------------------------------------------------ keyboard

  const moveEntry = useCallback(
    (to: (i: number) => number) => {
      const i = list.findIndex((e) => e.id === selected);
      const next = list[Math.max(0, Math.min(list.length - 1, to(i)))];
      if (next) setPicked(next.id);
    },
    [list, selected],
  );

  const moveGroup = useCallback(
    (to: (i: number) => number) => {
      const i = visible.findIndex((g) => g.id === groupId);
      const next = visible[Math.max(0, Math.min(visible.length - 1, to(i)))];
      if (next) selectGroup(next.id);
    },
    [visible, groupId, selectGroup],
  );

  useEffect(() => {
    const paneRefs = { groups: treeRef, list: listRef, detail: detailRef };
    const where = (): Pane | 'search' | 'body' | 'other' => {
      const el = document.activeElement;
      if (!el || el === document.body) return 'body';
      if (el === search.current) return 'search';
      for (const p of PANES) if (paneRefs[p].current?.contains(el)) return p;
      return 'other';
    };

    const onKey = (e: KeyboardEvent) => {
      const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      const ctrl = e.ctrlKey || e.metaKey;
      const at = where();
      const typing = at === 'search';
      const run = (fn: () => void) => {
        e.preventDefault();
        fn();
      };

      // a dialog answers its own keys
      if (ask || historyFor) return;

      // editing takes the keyboard: Escape cancels, Ctrl+S saves, the rest
      // (Tab between fields included) belongs to the form
      if (mode.kind !== 'view') {
        if (k === 'Escape') return run(closeEditor);
        if (ctrl && !e.shiftKey && k === 's') return run(() => runSave());
        return;
      }

      // Tab moves between the three panes (and from the search box into the list)
      if (k === 'Tab' && !ctrl && !e.altKey && at !== 'other') {
        const target: Pane =
          at === 'search' || at === 'body'
            ? e.shiftKey
              ? 'groups'
              : 'list'
            : (PANES[(PANES.indexOf(at) + (e.shiftKey ? 2 : 1)) % 3] ?? 'list');
        return run(() => paneRefs[target].current?.focus());
      }

      if (ctrl && e.altKey && k === 'k') return run(() => void api.showQuickSearch().catch(fail));
      if (e.altKey) return;
      if (ctrl && !e.shiftKey) {
        if (k === 'f') return run(() => search.current?.select());
        if (k === 'l') return run(lock);
        if (k === 'b') return run(() => detail?.username && actions.copy(Field.UserName));
        if (k === 'u') return run(() => detail?.url && actions.copy(Field.URL));
        if (k === 't') return run(() => detail?.hasTotp && actions.copy(Field.Totp));
        if (k === 'h') return run(() => detail?.hasPassword && actions.toggleReveal(Field.Password));
        if (k === 'e') return run(actions.edit);
        if (k === 'n') return run(startNew);
        if (k === 's') return run(() => runSave());
        if (k === ',')
          return run(() => document.querySelector<HTMLButtonElement>('[data-settings-toggle]')?.click());
        if (k === 'c') {
          // inside a text box with a selection, Ctrl+C copies the selection
          const el = e.target as HTMLInputElement | null;
          if (typing && el && el.selectionStart !== el.selectionEnd) return;
          if (window.getSelection()?.toString()) return;
          return run(() => detail?.hasPassword && actions.copy(Field.Password));
        }
      }
      if (ctrl && e.shiftKey && k === 'u') return run(actions.openUrl);
      if (ctrl && e.shiftKey && k === 'v') return run(actions.autotype);
      if (ctrl || e.shiftKey) return;

      if (k === 'Escape' && query) return run(() => setQuery(''));

      if (at === 'groups') {
        const g = groupId ? byId.get(groupId) : undefined;
        if (k === 'ArrowDown') return run(() => moveGroup((i) => i + 1));
        if (k === 'ArrowUp') return run(() => moveGroup((i) => i - 1));
        if (k === 'Home') return run(() => moveGroup(() => 0));
        if (k === 'End') return run(() => moveGroup(() => visible.length - 1));
        if (k === 'ArrowLeft' && g)
          return run(() =>
            g.hasChildren && !collapsed.has(g.id)
              ? setOpen(g.id, false)
              : g.parentId && selectGroup(g.parentId),
          );
        if (k === 'ArrowRight' && g?.hasChildren)
          return run(() => (collapsed.has(g.id) ? setOpen(g.id, true) : moveGroup((i) => i + 1)));
        if (k === 'Enter') return run(() => listRef.current?.focus());
        return;
      }

      if (at === 'list' || at === 'search' || at === 'body') {
        if (k === 'ArrowDown') return run(() => moveEntry((i) => i + 1));
        if (k === 'ArrowUp') return run(() => moveEntry((i) => i - 1));
        if (at === 'list') {
          if (k === 'Home') return run(() => moveEntry(() => 0));
          if (k === 'End') return run(() => moveEntry(() => list.length - 1));
          if (k === 'PageDown') return run(() => moveEntry((i) => i + PAGE));
          if (k === 'PageUp') return run(() => moveEntry((i) => i - PAGE));
        }
        if (k === 'Enter' && !typing && detail) return run(actions.edit);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [
    actions,
    ask,
    byId,
    closeEditor,
    collapsed,
    detail,
    fail,
    groupId,
    historyFor,
    list.length,
    lock,
    mode.kind,
    moveEntry,
    moveGroup,
    query,
    runSave,
    selectGroup,
    setOpen,
    startNew,
    t,
    toast,
    visible.length,
  ]);

  const listTitle = q ? t('search.results', { q }) : (byId.get(groupId ?? '')?.name ?? '');

  const searchBox = (
    <label className="search">
      <Icon name="search" />
      <input
        ref={search}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={t('search.placeholder')}
        aria-label={t('search.label')}
        title={t('search.hint')}
        spellCheck={false}
        autoComplete="off"
      />
      <Keys keys={['Ctrl', 'F']} />
    </label>
  );

  return (
    <div className="shell workspace">
      <TitleBar
        fileName={status.file ?? status.name}
        dirty={status.dirty}
        center={searchBox}
        onQuickSearch={() => void api.showQuickSearch().catch(fail)}
        onSave={status.dirty ? () => runSave() : undefined}
        onLock={lock}
      />
      <div className="body">
        <GroupNav
          visible={visible}
          selected={q ? null : groupId}
          collapsed={collapsed}
          onSelect={selectGroup}
          onToggle={toggle}
          treeRef={treeRef}
        />
        <div className="content-layer">
          <EntryList
            title={listTitle}
            entries={list}
            selected={selected}
            query={q}
            sort={settings.entrySort}
            onSort={sortBy}
            onSelect={setPicked}
            onNew={startNew}
            listRef={listRef}
          />
          <section className="detail pane" aria-live="polite" tabIndex={0} ref={detailRef}>
            {mode.kind === 'edit' ? (
              <EntryEditor
                key={mode.entry.id}
                entry={mode.entry}
                groupName={detail?.groupPath.join(' / ') ?? ''}
                busy={busy}
                onSave={submitEdit}
                onCancel={closeEditor}
              />
            ) : mode.kind === 'new' ? (
              <EntryEditor
                key="new"
                entry={null}
                groupName={targetGroup?.name ?? ''}
                busy={busy}
                onSave={submitEdit}
                onCancel={closeEditor}
              />
            ) : detail ? (
              <EntryDetail entry={detail} revealed={revealed} copied={copied} actions={actions} />
            ) : (
              <div className="empty">{t('detail.empty')}</div>
            )}
          </section>
        </div>
      </div>

      {historyFor && (
        <HistoryDialog
          entryId={historyFor}
          onClose={() => setHistoryFor(null)}
          onRestored={() => {
            setHistoryFor(null);
            setVersion((v) => v + 1);
            toast({ message: t('toast.restored') });
          }}
          onError={fail}
        />
      )}

      {ask?.kind === 'lock' && (
        <Dialog
          title={t('dialog.unsaved.title')}
          onClose={() => setAsk(null)}
          actions={[
            { label: t('dialog.unsaved.cancel'), onClick: () => setAsk(null) },
            {
              label: t('dialog.unsaved.discard'),
              danger: true,
              onClick: () => {
                setAsk(null);
                api.lock(true).catch(fail);
              },
            },
            {
              label: t('dialog.unsaved.save'),
              primary: true,
              onClick: () => {
                afterSave.current = 'lock';
                runSave();
              },
            },
          ]}
        >
          <p>{t('dialog.unsaved.message')}</p>
        </Dialog>
      )}

      {ask?.kind === 'quit' && (
        <Dialog
          title={t('dialog.unsaved.title')}
          onClose={() => setAsk(null)}
          actions={[
            { label: t('dialog.unsaved.cancel'), onClick: () => setAsk(null) },
            {
              label: t('dialog.unsaved.discard'),
              danger: true,
              onClick: () => {
                setAsk(null);
                quit(true);
              },
            },
            {
              label: t('dialog.unsaved.save'),
              primary: true,
              onClick: () => {
                afterSave.current = 'quit';
                runSave();
              },
            },
          ]}
        >
          <p>{t('dialog.unsaved.message')}</p>
        </Dialog>
      )}

      {ask?.kind === 'conflict' && (
        <Dialog
          title={t('dialog.conflict.title')}
          onClose={() => setAsk(null)}
          actions={[
            { label: t('dialog.conflict.cancel'), onClick: () => setAsk(null) },
            {
              label: t('dialog.conflict.reload'),
              danger: true,
              onClick: () => {
                setAsk(null);
                afterSave.current = null;
                api.reloadDatabase().then(() => {
                  setVersion((v) => v + 1);
                  toast({ message: t('toast.reloaded') });
                }, fail);
              },
            },
            {
              label: t('dialog.conflict.overwrite'),
              primary: true,
              onClick: () => runSave({ force: true }),
            },
          ]}
        >
          <p>{t('dialog.conflict.message')}</p>
        </Dialog>
      )}

      {ask?.kind === 'upgrade' && (
        <Dialog
          title={t('dialog.upgrade.title')}
          onClose={() => setAsk(null)}
          actions={[
            { label: t('dialog.unsaved.cancel'), onClick: () => setAsk(null) },
            {
              label: t('dialog.upgrade.save'),
              primary: true,
              onClick: () => runSave({ upgrade: true }),
            },
          ]}
        >
          <p>{t('dialog.upgrade.message')}</p>
        </Dialog>
      )}
    </div>
  );
}
