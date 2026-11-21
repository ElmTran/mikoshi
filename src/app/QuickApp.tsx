import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { api, isTauri, listen } from '../api';
import { appWindow } from '../api/window';
import {
  AppError,
  Field,
  type EntrySummary,
  type QuickMode,
  type QuickShown,
  type VaultStatus,
} from '../api/types';
import { EntryIcon, Keys } from '../components/bits';
import { Icon } from '../components/Icon';
import { UnlockForm } from '../components/Unlock';
import { Highlight } from '../components/Highlight';
import { host, searchTerms } from '../text';
import { usePrefs } from './contexts';
import { CustomIconsProvider } from './CustomIcons';
import { QuickSave } from './QuickSave';

type Action = 'autotype' | 'typePassword' | 'password' | 'username' | 'totp';
type Copy = 'password' | 'username' | 'totp';

/** The quick search box (its own borderless window). */
export function QuickApp() {
  const { t } = usePrefs();
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<EntrySummary[]>([]);
  const [index, setIndex] = useState(0);
  const [flash, setFlash] = useState<{ id: string; text: string; ok: boolean } | null>(null);
  const [opened, setOpened] = useState(0);
  // counts edits, so the results are searched again
  const [edits, setEdits] = useState(0);
  // what the box was opened for (in a browser: ?mode=save, for UI work)
  const [mode, setMode] = useState<QuickMode>(() =>
    !isTauri && new URLSearchParams(location.search).get('mode') === 'save' ? 'save' : 'search',
  );
  // auto-type hotkey: list the entries that fit the window behind
  const [pick, setPick] = useState(false);
  // the right-click menu: which entry, and where (relative to the panel)
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const hideTimer = useRef(0);

  /** `restore`: focus goes back to the window in front before (not after a click elsewhere) */
  const hide = useCallback((restore = true) => {
    window.clearTimeout(hideTimer.current);
    if (isTauri) void api.hideQuickSearch(restore).catch(() => {});
  }, []);

  useEffect(() => {
    const loadStatus = () => api.vaultStatus().then(setStatus, () => {});
    void loadStatus();
    const offs = [
      listen<VaultStatus>('vault-changed', setStatus),
      // an edit changes what the search finds
      // (not a remount: that would also reset a quick save form)
      listen<VaultStatus>('vault-edited', () => setEdits((n) => n + 1)),
      listen<QuickShown>('quick-shown', (shown) => {
        // a hide still pending from the last time must not close this one
        window.clearTimeout(hideTimer.current);
        // ask again each time: a failed first call must not leave the box empty
        void loadStatus();
        setQuery('');
        setIndex(0);
        setFlash(null);
        setMenu(null);
        setMode(shown?.mode ?? 'search');
        setPick(shown?.mode === 'pick');
        // remounts the panel, which replays its entrance and refocuses the input
        setOpened((n) => n + 1);
      }),
      // Clicking anywhere else closes the box, like a menu. The Rust side
      // does that (lib.rs, on_window_event): this page sleeps while hidden,
      // and a focus event queued then would arrive on the next show and
      // close the box at once.
    ];
    return () => offs.forEach((p) => void p.then((f) => f()));
  }, []);

  // Windows 11: the window is the frosted panel, drawn by Windows; its
  // height follows the content (windows::quick_glass)
  const [glass, setGlass] = useState(false);
  useEffect(() => {
    if (!isTauri) return;
    api.appInfo().then(
      (info) => {
        document.documentElement.classList.toggle('glass', info.quickGlass);
        setGlass(info.quickGlass);
      },
      () => {},
    );
  }, []);
  useEffect(() => {
    const el = panel.current;
    if (!glass || !el) return;
    let last = 0;
    const fit = () => {
      const height = Math.ceil(el.getBoundingClientRect().height);
      if (height > 0 && Math.abs(height - last) > 1) {
        last = height;
        void appWindow.setSize(window.innerWidth, height).catch(() => {});
      }
    };
    const watch = new ResizeObserver(fit);
    watch.observe(el);
    fit();
    return () => watch.disconnect();
  }, [glass, opened]);

  const q = query.trim();
  const terms = useMemo(() => searchTerms(q), [q]);
  const unlocked = status !== null && !status.locked;
  // picking only until the user starts typing a search
  const picking = pick && !q;
  useEffect(() => {
    if (!unlocked || mode === 'save') return;
    let live = true;
    if (picking) {
      api.autotypeCandidates().then(
        (list) => {
          if (!live) return;
          // nothing fits the window: an ordinary search box
          if (list.length === 0) setPick(false);
          else {
            setResults(list);
            setIndex(0);
          }
        },
        () => live && setPick(false),
      );
      return () => {
        live = false;
      };
    }
    api.searchEntries(q).then(
      (list) => {
        if (!live) return;
        setResults(list.slice(0, q ? 8 : 6));
        setIndex(0);
      },
      () => live && setResults([]),
    );
    return () => {
      live = false;
    };
  }, [q, unlocked, picking, mode, edits]);

  const showError = (id: string, err: unknown) =>
    setFlash({ id, text: t(`error.${err instanceof AppError ? err.code : 'internal'}`), ok: false });

  const run = (action: Action, entry: EntrySummary | undefined = results[index]) => {
    const e = entry;
    setMenu(null);
    if (!e) return;
    if (action === 'autotype' || action === 'typePassword') {
      // the Rust side hides the box and types into the window behind it
      api.autotypeEntry(e.id, action === 'typePassword').catch((err: unknown) => showError(e.id, err));
      return;
    }
    if (action === 'totp' && !e.hasTotp) {
      setFlash({ id: e.id, text: t('quick.noTotp'), ok: false });
      return;
    }
    const key = action === 'password' ? Field.Password : action === 'username' ? Field.UserName : Field.Totp;
    api.copyField(e.id, key).then(
      () => {
        setFlash({ id: e.id, text: t('quick.copied'), ok: true });
        // copy, show the tick for a moment, then get out of the way
        hideTimer.current = window.setTimeout(() => hide(), 450);
      },
      (err: unknown) => showError(e.id, err),
    );
  };

  const openMenu = (id: string, clientX: number, clientY: number) => {
    const r = panel.current?.getBoundingClientRect();
    if (!r) return;
    // keep the menu inside the panel (it clips what sticks out)
    const x = Math.min(clientX - r.left, r.width - 196);
    const y = Math.min(clientY - r.top, r.height - 124);
    setMenu({ id, x: Math.max(6, x), y: Math.max(6, y) });
  };

  const onKey = (e: KeyboardEvent) => {
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const handled = (fn: () => void) => {
      e.preventDefault();
      fn();
    };
    // the quick save form has keys of its own
    if (mode === 'save' && unlocked) return;
    if (k === 'Escape') return handled(() => (menu ? setMenu(null) : hide()));
    if (!unlocked) return;
    if (menu) return;
    if (k === 'ArrowDown') return handled(() => setIndex((i) => Math.min(results.length - 1, i + 1)));
    if (k === 'ArrowUp') return handled(() => setIndex((i) => Math.max(0, i - 1)));
    if (k === 'Enter') return handled(() => run(ctrl ? 'typePassword' : 'autotype'));
    if (ctrl && k === 'c') {
      // with text selected in the search box, Ctrl+C copies the text
      const el = e.target as HTMLInputElement;
      if (el.selectionStart !== el.selectionEnd) return;
      return handled(() => run('password'));
    }
    if (ctrl && k === 'b') return handled(() => run('username'));
    if (ctrl && k === 't') return handled(() => run('totp'));
    // the keyboard's menu key, or Shift+F10: the menu for the selected entry
    if (k === 'ContextMenu' || (e.shiftKey && k === 'F10')) {
      const row = panel.current?.querySelector<HTMLElement>('.qr.on');
      const cur = results[index];
      if (row && cur) {
        const b = row.getBoundingClientRect();
        handled(() => openMenu(cur.id, b.left + 48, b.bottom - 6));
      }
    }
  };

  if (!status) return null;

  return (
    <CustomIconsProvider unlocked={unlocked}>
      <div className="quick-stage" onKeyDown={onKey}>
        <div
          className="qs"
          key={opened}
          ref={panel}
          role="dialog"
          aria-label={t(mode === 'save' ? 'quickSave.heading' : 'toolbar.quickSearch')}
          onPointerDown={(e) => {
            if (menu && !(e.target as Element).closest('.qr-menu')) setMenu(null);
          }}
        >
          {!status.path ? (
            <div className="qs-locked">
              <p className="qs-empty">{t('quick.noDatabase')}</p>
            </div>
          ) : status.locked ? (
            <UnlockForm
              status={status}
              compact
              placeholder={mode === 'save' ? t('quickSave.locked') : undefined}
            />
          ) : mode === 'save' ? (
            <QuickSave
              status={status}
              onClose={() => hide()}
              onDone={(ms) => {
                hideTimer.current = window.setTimeout(() => hide(), ms);
              }}
            />
          ) : (
            <>
              <label className="qs-input">
                <Icon name="search" size={20} />
                <input
                  ref={input}
                  autoFocus
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder={t('quick.placeholder')}
                  aria-label={t('quick.placeholder')}
                  spellCheck={false}
                  autoComplete="off"
                />
                <span className="qs-db">
                  <Icon name="vault" />
                  {status.name}
                </span>
              </label>
              {!q && results.length > 0 && (
                <div className="qs-caption">{t(picking ? 'quick.pick' : 'quick.recent')}</div>
              )}
              <div className="qs-results" role="listbox">
                {results.map((e, i) => {
                  // show the site instead of the user name when that is what matched
                  const hits = (s: string) => terms.some((w) => s.toLowerCase().includes(w.toLowerCase()));
                  const sub = terms.length && !hits(e.username) && hits(e.url) ? host(e.url) : e.username;
                  const flashing = flash?.id === e.id ? flash : null;
                  return (
                    <div
                      key={e.id}
                      className={i === index ? 'qr on' : 'qr'}
                      role="option"
                      aria-selected={i === index}
                      onMouseMove={() => !menu && setIndex(i)}
                      onDoubleClick={() => run('autotype', e)}
                      onContextMenu={(ev) => {
                        ev.preventDefault();
                        setIndex(i);
                        openMenu(e.id, ev.clientX, ev.clientY);
                      }}
                    >
                      <EntryIcon icon={e.icon} customIcon={e.customIcon} title={e.title} />
                      <span className="qr-main">
                        <span className="qr-title">
                          <Highlight text={e.title} terms={terms} />
                        </span>
                        <span className="qr-sub">
                          <Highlight text={sub} terms={terms} />
                        </span>
                      </span>
                      {flashing ? (
                        <span className={flashing.ok ? 'qr-flash ok' : 'qr-flash'}>
                          <Icon name={flashing.ok ? 'check' : 'warn'} size={14} />
                          {flashing.text}
                        </span>
                      ) : (
                        e.hasTotp && (
                          <span className="qr-totp" title={t('quick.hasTotp')}>
                            <Icon name="clock" size={14} />
                          </span>
                        )
                      )}
                    </div>
                  );
                })}
                {results.length === 0 && q && <div className="qs-empty">{t('quick.none', { q })}</div>}
              </div>
              {menu && (
                <QuickMenu
                  at={menu}
                  entry={results.find((r) => r.id === menu.id)}
                  onPick={(what, entry) => run(what, entry)}
                />
              )}
              <footer className="qs-foot">
                <span className="hint">
                  <Keys keys={['Enter']} />
                  {t('quick.autotype')}
                </span>
                <span className="hint">
                  <Keys keys={['Ctrl', 'Enter']} />
                  {t('quick.typePassword')}
                </span>
                <span className="hint">
                  <Keys keys={['Ctrl', 'C']} />
                  {t('quick.copyPassword')}
                </span>
                <span className="hint">
                  <Keys keys={[t('quick.rightClick')]} />
                  {t('quick.more')}
                </span>
                <span className="hint">
                  <Keys keys={['Esc']} />
                  {t('quick.close')}
                </span>
              </footer>
            </>
          )}
        </div>
      </div>
    </CustomIconsProvider>
  );
}

/** Right-click menu on a result: the three copies, with their shortcuts. */
function QuickMenu({
  at,
  entry,
  onPick,
}: {
  at: { x: number; y: number };
  entry: EntrySummary | undefined;
  onPick: (what: Copy, entry: EntrySummary) => void;
}) {
  const { t } = usePrefs();
  const first = useRef<HTMLButtonElement>(null);
  useEffect(() => first.current?.focus(), []);
  if (!entry) return null;
  const items: { what: Copy; label: string; keys: string[]; off?: boolean }[] = [
    { what: 'username', label: t('quick.copyUsername'), keys: ['Ctrl', 'B'], off: !entry.username },
    { what: 'password', label: t('quick.copyPassword'), keys: ['Ctrl', 'C'] },
    { what: 'totp', label: t('quick.copyTotp'), keys: ['Ctrl', 'T'], off: !entry.hasTotp },
  ];
  return (
    <div className="flyout qr-menu" role="menu" style={{ left: at.x, top: at.y }}>
      {items.map((it, i) => (
        <button
          key={it.what}
          ref={i === 1 ? first : undefined}
          type="button"
          role="menuitem"
          className="menu-item"
          disabled={it.off}
          onClick={() => onPick(it.what, entry)}
          onKeyDown={(e) => {
            const all = [
              ...(e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('button:enabled') ?? []),
            ];
            const pos = all.indexOf(e.currentTarget);
            const step = e.key === 'ArrowDown' ? 1 : e.key === 'ArrowUp' ? -1 : 0;
            if (step) {
              e.preventDefault();
              e.stopPropagation();
              all[(pos + step + all.length) % all.length]?.focus();
            } else if (e.key === 'Enter' || e.key === ' ') {
              // the button's own click, not the box's auto-type
              e.stopPropagation();
            }
          }}
        >
          <span>{it.label}</span>
          <Keys keys={it.keys} />
        </button>
      ))}
    </div>
  );
}
