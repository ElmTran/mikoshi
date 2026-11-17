import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react';
import { api } from '../api';
import { AppError, type VaultStatus } from '../api/types';
import { usePrefs, useToast } from '../app/contexts';
import { AppMark } from './bits';
import { Icon } from './Icon';

interface Props {
  status: VaultStatus;
  /** compact form for the quick search box */
  compact?: boolean;
  onUnlocked?: () => void;
}

export function UnlockForm({ status, compact, onUnlocked }: Props) {
  const { t, info } = usePrefs();
  const toast = useToast();
  const [password, setPassword] = useState('');
  const [show, setShow] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [caps, setCaps] = useState(false);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => input.current?.focus(), []);

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
      await api.unlock(password);
      setPassword('');
      onUnlocked?.();
    } catch (err) {
      const code = err instanceof AppError ? err.code : 'internal';
      setError(t(`error.${code}`));
      shake();
      input.current?.select();
    } finally {
      setBusy(false);
    }
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => setCaps(e.getModifierState('CapsLock'));

  return (
    <form className={compact ? 'unlock-form compact' : 'unlock-form'} onSubmit={submit} autoComplete="off">
      {!compact && <h1 className="unlock-title">{t('unlock.title')}</h1>}
      {!compact && (
        <div className="unlock-file">
          <Icon name="vault" size={20} />
          <span className="unlock-file-text">
            <span className="unlock-file-name">{status.file}</span>
            <span className="unlock-file-path">{status.path}</span>
          </span>
          <Icon name="chevron" />
        </div>
      )}
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
            {error}
          </span>
        ) : caps ? (
          <span className="caps">{t('unlock.capsLock')}</span>
        ) : info?.demo ? (
          <span className="hint">{t('unlock.demoHint')}</span>
        ) : null}
      </div>
      {!compact && (
        <button
          type="button"
          className="link-btn"
          onClick={() => toast({ message: t('unlock.keyfile'), tone: 'info' })}
        >
          <Icon name="key" />
          {t('unlock.keyfile')}
        </button>
      )}
      <button type="submit" className="btn primary unlock-submit" disabled={busy}>
        {t('unlock.submit')}
      </button>
    </form>
  );
}

/** The locked state of the main window. */
export function UnlockScreen({ status }: { status: VaultStatus }) {
  return (
    <main className="unlock-screen">
      <div className="unlock-hero">
        <AppMark size={64} />
      </div>
      <div className="unlock-card">
        <UnlockForm status={status} />
      </div>
    </main>
  );
}
