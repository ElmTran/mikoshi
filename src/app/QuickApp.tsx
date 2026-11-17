import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { api, isTauri, listen } from '../api';
import { AppError, Field, type EntrySummary, type VaultStatus } from '../api/types';
import { appWindow } from '../api/window';
import { Avatar, Keys } from '../components/bits';
import { Icon } from '../components/Icon';
import { UnlockForm } from '../components/Unlock';
import { usePrefs } from './contexts';

type Action = 'password' | 'username' | 'totp' | 'autotype';

const host = (url: string) => url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '').split('/')[0] ?? url;

function Highlight({ text, query }: { text: string; query: string }) {
  const i = query ? text.toLowerCase().indexOf(query.toLowerCase()) : -1;
  if (i < 0) return <>{text}</>;
  return (
    <>
      {text.slice(0, i)}
      <mark>{text.slice(i, i + query.length)}</mark>
      {text.slice(i + query.length)}
    </>
  );
}

/** The quick search box (its own borderless window). */
export function QuickApp() {
  const { t } = usePrefs();
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<EntrySummary[]>([]);
  const [index, setIndex] = useState(0);
  const [flash, setFlash] = useState<{ id: string; text: string; ok: boolean } | null>(null);
  const [opened, setOpened] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const hideTimer = useRef(0);

  const hide = useCallback(() => {
    window.clearTimeout(hideTimer.current);
    if (isTauri) void api.hideQuickSearch().catch(() => {});
  }, []);

  useEffect(() => {
    const loadStatus = () => api.vaultStatus().then(setStatus, () => {});
    void loadStatus();
    const offs = [
      listen<VaultStatus>('vault-changed', setStatus),
      listen('quick-shown', () => {
        // ask again each time: a failed first call must not leave the box empty
        void loadStatus();
        setQuery('');
        setIndex(0);
        setFlash(null);
        // remounts the panel, which replays its entrance and refocuses the input
        setOpened((n) => n + 1);
      }),
      // like a menu: clicking anywhere else closes it
      appWindow.onFocusChanged((focused) => {
        if (!focused) hide();
      }),
    ];
    return () => offs.forEach((p) => void p.then((f) => f()));
  }, [hide]);

  const q = query.trim();
  const unlocked = status !== null && !status.locked;
  useEffect(() => {
    if (!unlocked) return;
    let live = true;
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
  }, [q, unlocked]);

  const run = (action: Action) => {
    const e = results[index];
    if (!e) return;
    if (action === 'autotype') {
      setFlash({ id: e.id, text: t('toast.autotype'), ok: false });
      hideTimer.current = window.setTimeout(hide, 900);
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
        hideTimer.current = window.setTimeout(hide, 450);
      },
      (err: unknown) =>
        setFlash({
          id: e.id,
          text: t(`error.${err instanceof AppError ? err.code : 'internal'}`),
          ok: false,
        }),
    );
  };

  const onKey = (e: KeyboardEvent) => {
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const handled = (fn: () => void) => {
      e.preventDefault();
      fn();
    };
    if (k === 'Escape') return handled(hide);
    if (!unlocked) return;
    if (k === 'ArrowDown') return handled(() => setIndex((i) => Math.min(results.length - 1, i + 1)));
    if (k === 'ArrowUp') return handled(() => setIndex((i) => Math.max(0, i - 1)));
    if (k === 'Enter') return handled(() => run(ctrl ? 'autotype' : 'password'));
    if (ctrl && k === 'b') return handled(() => run('username'));
    if (ctrl && k === 't') return handled(() => run('totp'));
  };

  if (!status) return null;

  return (
    <div className="quick-stage" onKeyDown={onKey}>
      <div className="qs" key={opened} role="dialog" aria-label={t('toolbar.quickSearch')}>
        {status.locked ? (
          <div className="qs-locked">
            <UnlockForm status={status} compact />
          </div>
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
            {!q && results.length > 0 && <div className="qs-caption">{t('quick.recent')}</div>}
            <div className="qs-results" role="listbox">
              {results.map((e, i) => {
                const sub =
                  q &&
                  !e.username.toLowerCase().includes(q.toLowerCase()) &&
                  e.url.toLowerCase().includes(q.toLowerCase())
                    ? host(e.url)
                    : e.username;
                const flashing = flash?.id === e.id ? flash : null;
                return (
                  <div
                    key={e.id}
                    className={i === index ? 'qr on' : 'qr'}
                    role="option"
                    aria-selected={i === index}
                    onMouseMove={() => setIndex(i)}
                    onDoubleClick={() => run('password')}
                  >
                    <Avatar title={e.title} />
                    <span className="qr-main">
                      <span className="qr-title">
                        <Highlight text={e.title} query={q} />
                      </span>
                      <span className="qr-sub">
                        <Highlight text={sub} query={q} />
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
            <footer className="qs-foot">
              <span className="hint">
                <Keys keys={['Enter']} />
                {t('quick.copyPassword')}
              </span>
              <span className="hint">
                <Keys keys={['Ctrl', 'Enter']} />
                {t('quick.autotype')}
              </span>
              <span className="hint">
                <Keys keys={['Ctrl', 'B']} />
                {t('quick.copyUsername')}
              </span>
              <span className="hint">
                <Keys keys={['Ctrl', 'T']} />
                {t('quick.copyTotp')}
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
  );
}
