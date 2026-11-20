import { useCallback, useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react';
import { api } from '../api';
import { AppError, type DialogText, type VaultStatus } from '../api/types';
import { usePrefs } from '../app/contexts';
import type { MessageKey } from '../i18n/core';
import { AppMark } from './bits';
import { Icon } from './Icon';
import { NewDatabaseForm } from './NewDatabase';
import { useDismiss } from './useDismiss';
import { baseName } from '../text';

const errorKey = (e: unknown): MessageKey => `error.${e instanceof AppError ? e.code : 'internal'}`;

/** Windows paths are case-insensitive. */
const samePath = (a: string | null, b: string | null) =>
  Boolean(a && b && a.toLowerCase() === b.toLowerCase());

function useDialogTexts() {
  const { t } = usePrefs();
  const database: DialogText = {
    title: t('dialog.database'),
    filter: t('dialog.databaseFilter'),
    allFiles: t('dialog.allFiles'),
  };
  const keyFile: DialogText = {
    title: t('dialog.keyFile'),
    filter: t('dialog.keyFileFilter'),
    allFiles: t('dialog.allFiles'),
  };
  return { database, keyFile };
}

/** The selected database, and a menu to switch: recent files or another file. */
function DatabasePicker({ status, onError }: { status: VaultStatus; onError: (key: MessageKey) => void }) {
  const { t, settings } = usePrefs();
  const texts = useDialogTexts();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, root, close);

  const pick = (path: string) => {
    setOpen(false);
    if (!samePath(path, status.path)) api.selectDatabase(path).catch((e: unknown) => onError(errorKey(e)));
  };
  const browse = () => {
    setOpen(false);
    api.chooseDatabase(texts.database).catch((e: unknown) => onError(errorKey(e)));
  };
  const forget = (path: string) => api.forgetRecent(path).catch((e: unknown) => onError(errorKey(e)));

  return (
    <div className="db-picker" ref={root}>
      <button
        type="button"
        className="unlock-file"
        aria-haspopup="menu"
        aria-expanded={open}
        title={t('unlock.switchDatabase')}
        onClick={() => setOpen((o) => !o)}
      >
        <Icon name="vault" size={20} />
        <span className="unlock-file-text">
          <span className="unlock-file-name">{status.file}</span>
          <span className="unlock-file-path">{status.path}</span>
        </span>
        <Icon name="chevron" className={open ? 'flip' : undefined} />
      </button>
      {open && (
        <div className="flyout db-menu" role="menu" aria-label={t('unlock.switchDatabase')}>
          {settings.recent.length > 0 && <div className="flyout-label">{t('unlock.recent')}</div>}
          {settings.recent.map((r) => (
            <div
              key={r.path}
              className={samePath(r.path, status.path) ? 'db-item on' : 'db-item'}
              role="menuitem"
              tabIndex={0}
              onClick={() => pick(r.path)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  pick(r.path);
                }
              }}
            >
              <Icon name="vault" />
              <span className="db-item-text">
                <span className="db-item-name">{baseName(r.path)}</span>
                <span className="db-item-path">{r.path}</span>
              </span>
              <button
                type="button"
                className="act"
                title={t('unlock.forget')}
                aria-label={t('unlock.forget')}
                onClick={(e) => {
                  e.stopPropagation();
                  void forget(r.path);
                }}
              >
                <Icon name="close" size={14} />
              </button>
            </div>
          ))}
          <button type="button" className="db-item browse" role="menuitem" onClick={browse}>
            <Icon name="folderOpen" />
            <span className="db-item-name">{t('unlock.browse')}</span>
          </button>
        </div>
      )}
    </div>
  );
}

interface Props {
  status: VaultStatus;
  /** compact form for the quick search box: password only */
  compact?: boolean;
  onUnlocked?: () => void;
  /** "New database…" (main window only) */
  onNew?: () => void;
}

export function UnlockForm({ status, compact, onUnlocked, onNew }: Props) {
  const { t, info } = usePrefs();
  const texts = useDialogTexts();
  const [password, setPassword] = useState('');
  const [show, setShow] = useState(false);
  const [error, setError] = useState<MessageKey | null>(null);
  const [caps, setCaps] = useState(false);
  const [busy, setBusy] = useState(false);
  const [keyFile, setKeyFile] = useState(status.keyFile);
  // another database brings its own remembered key file
  const [shownPath, setShownPath] = useState(status.path);
  if (shownPath !== status.path) {
    setShownPath(status.path);
    setKeyFile(status.keyFile);
    setError(null);
    // a password typed for one database must not be tried on another
    setPassword('');
    setShow(false);
  }
  const input = useRef<HTMLInputElement>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => input.current?.focus(), [status.path]);

  /** A short sideways shake on a failed attempt; the input keeps its focus. */
  const shake = () => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    box.current?.animate(
      [0, -6, 5, -3, 2, 0].map((x) => ({ transform: `translateX(${x}px)` })),
      { duration: 360, easing: 'cubic-bezier(0.2, 0, 0, 1)' },
    );
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    try {
      // the quick box has no key file control: it uses the remembered one
      await api.unlock(password, compact ? status.keyFile : keyFile);
      setPassword('');
      onUnlocked?.();
    } catch (err) {
      setError(errorKey(err));
      shake();
      input.current?.select();
    } finally {
      setBusy(false);
    }
  };

  const chooseKeyFile = () =>
    api.chooseKeyFile(texts.keyFile).then(
      (path) => {
        if (path) {
          setKeyFile(path);
          setError(null);
        }
      },
      (e: unknown) => setError(errorKey(e)),
    );

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => setCaps(e.getModifierState('CapsLock'));

  return (
    <form className={compact ? 'unlock-form compact' : 'unlock-form'} onSubmit={submit} autoComplete="off">
      {!compact && <h1 className="unlock-title">{t('unlock.title')}</h1>}
      {!compact && <DatabasePicker status={status} onError={setError} />}
      <label className="unlock-label" htmlFor="master-password">
        {compact ? t('quick.locked') : t('unlock.password')}
      </label>
      <div className="unlock-input" ref={box}>
        <input
          id="master-password"
          ref={input}
          type={show ? 'text' : 'password'}
          value={password}
          onChange={(e) => {
            setPassword(e.target.value);
            setError(null);
          }}
          onKeyDown={onKey}
          onKeyUp={onKey}
          spellCheck={false}
          autoComplete="off"
          aria-invalid={Boolean(error)}
          aria-describedby="unlock-msg"
        />
        <button
          type="button"
          className="act"
          title={t(show ? 'unlock.hidePassword' : 'unlock.showPassword')}
          aria-label={t(show ? 'unlock.hidePassword' : 'unlock.showPassword')}
          aria-pressed={show}
          onClick={() => setShow((s) => !s)}
        >
          <Icon name={show ? 'eyeOff' : 'eye'} />
        </button>
      </div>
      <div className="unlock-msg" id="unlock-msg" role="alert">
        {error ? (
          <span className="error">
            <Icon name="warn" />
            {t(error)}
          </span>
        ) : caps ? (
          <span className="caps">{t('unlock.capsLock')}</span>
        ) : info?.demo ? (
          <span className="hint">{t('unlock.demoHint')}</span>
        ) : null}
      </div>
      {!compact &&
        (keyFile ? (
          <div className="unlock-keyfile" title={keyFile}>
            <Icon name="key" />
            <span className="unlock-keyfile-text">
              <span className="unlock-keyfile-label">{t('unlock.keyFileLabel')}</span>
              <span className="unlock-keyfile-name">{baseName(keyFile)}</span>
            </span>
            <button
              type="button"
              className="act"
              title={t('unlock.changeKeyFile')}
              aria-label={t('unlock.changeKeyFile')}
              onClick={() => void chooseKeyFile()}
            >
              <Icon name="edit" size={14} />
            </button>
            <button
              type="button"
              className="act"
              title={t('unlock.removeKeyFile')}
              aria-label={t('unlock.removeKeyFile')}
              onClick={() => setKeyFile(null)}
            >
              <Icon name="close" size={14} />
            </button>
          </div>
        ) : (
          <button type="button" className="link-btn" onClick={() => void chooseKeyFile()}>
            <Icon name="key" />
            {t('unlock.keyfile')}
          </button>
        ))}
      <button type="submit" className="btn primary unlock-submit" disabled={busy}>
        {busy ? t('unlock.unlocking') : t('unlock.submit')}
      </button>
      {onNew && (
        <button type="button" className="link-btn new-db-link" onClick={onNew}>
          <Icon name="plus" />
          {t('newdb.link')}
        </button>
      )}
    </form>
  );
}

/** First run, or every remembered file was removed: choose a database. */
function Welcome({ onNew }: { onNew: () => void }) {
  const { t } = usePrefs();
  const texts = useDialogTexts();
  const [error, setError] = useState<MessageKey | null>(null);
  const browse = () => api.chooseDatabase(texts.database).catch((e: unknown) => setError(errorKey(e)));

  return (
    <div className="unlock-form welcome">
      <h1 className="unlock-title">{t('unlock.welcome')}</h1>
      <p className="unlock-hint">{t('unlock.welcomeHint')}</p>
      <div className="unlock-msg" role="alert">
        {error && (
          <span className="error">
            <Icon name="warn" />
            {t(error)}
          </span>
        )}
      </div>
      <button type="button" className="btn primary unlock-submit" onClick={() => void browse()} autoFocus>
        <Icon name="folderOpen" />
        {t('unlock.openFile')}
      </button>
      <button type="button" className="link-btn new-db-link" onClick={onNew}>
        <Icon name="plus" />
        {t('newdb.link')}
      </button>
    </div>
  );
}

/** The locked state of the main window. */
export function UnlockScreen({ status }: { status: VaultStatus }) {
  const [creating, setCreating] = useState(false);
  const startNew = () => setCreating(true);
  return (
    <main className="unlock-screen">
      <div className="unlock-hero">
        <AppMark size={64} />
      </div>
      <div className="unlock-card">
        {creating ? (
          <NewDatabaseForm onBack={() => setCreating(false)} />
        ) : status.path ? (
          <UnlockForm status={status} onNew={startNew} />
        ) : (
          <Welcome onNew={startNew} />
        )}
      </div>
    </main>
  );
}
