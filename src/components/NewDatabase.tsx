import { useState, type FormEvent } from 'react';
import { api } from '../api';
import { AppError, type DialogText } from '../api/types';
import { usePrefs } from '../app/contexts';
import type { MessageKey } from '../i18n/core';
import { baseName } from '../text';
import { Icon } from './Icon';
import { StrengthMeter } from './Strength';

const errorKey = (e: unknown): MessageKey => `error.${e instanceof AppError ? e.code : 'internal'}`;

/** File names cannot hold these on Windows, nor control characters. */
const safeFileName = (name: string) =>
  [...name]
    .filter((c) => !'<>:"/\\|?*'.includes(c) && c.charCodeAt(0) >= 32)
    .join('')
    .trim() || 'Passwords';

/**
 * A new database: name, master password (twice), an optional key file
 * (an existing one, or a new one made here). Creating asks where to save it,
 * then calibrates the KDF to about a second on this computer and opens it.
 */
export function NewDatabaseForm({ onBack }: { onBack: () => void }) {
  const { t } = usePrefs();
  const [name, setName] = useState('');
  const [password, setPassword] = useState('');
  const [again, setAgain] = useState('');
  const [show, setShow] = useState(false);
  const [keyFile, setKeyFile] = useState<string | null>(null);
  const [error, setError] = useState<MessageKey | null>(null);
  const [busy, setBusy] = useState(false);

  const dialog = (title: string, filter: string): DialogText => ({
    title,
    filter,
    allFiles: t('dialog.allFiles'),
  });
  const mismatch = password !== '' && again !== '' && password !== again;

  const chooseKeyFile = () =>
    api.chooseKeyFile(dialog(t('dialog.keyFile'), t('dialog.keyFileFilter'))).then(
      (p) => p && setKeyFile(p),
      (e: unknown) => setError(errorKey(e)),
    );
  const makeKeyFile = () =>
    api
      .createKeyFile(dialog(t('dialog.newKeyFile'), t('dialog.keyFileFilter')), `${safeFileName(name)}.keyx`)
      .then(
        (p) => p && setKeyFile(p),
        (e: unknown) => setError(errorKey(e)),
      );

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    if (!password && !keyFile) return setError('newdb.needKey');
    if (password !== again) return setError('newdb.mismatch');
    setError(null);
    try {
      const path = await api.chooseNewDatabase(
        dialog(t('dialog.newDatabase'), t('dialog.databaseFilter')),
        `${safeFileName(name)}.kdbx`,
      );
      if (!path) return;
      setBusy(true);
      // opens it on success; the window switches to the workspace
      await api.createDatabase(path, name.trim(), password, keyFile);
    } catch (err) {
      setError(errorKey(err));
      setBusy(false);
    }
  };

  return (
    <form className="unlock-form new-db" onSubmit={(e) => void submit(e)} autoComplete="off">
      <h1 className="unlock-title">{t('newdb.title')}</h1>
      <label className="unlock-label" htmlFor="newdb-name">
        {t('newdb.name')}
      </label>
      <div className="unlock-input">
        <input
          id="newdb-name"
          value={name}
          placeholder={t('newdb.namePlaceholder')}
          onChange={(e) => setName(e.target.value)}
          spellCheck={false}
          autoFocus
        />
      </div>

      <label className="unlock-label" htmlFor="newdb-password">
        {t('newdb.password')}
      </label>
      <div className="unlock-input">
        <input
          id="newdb-password"
          type={show ? 'text' : 'password'}
          value={password}
          onChange={(e) => {
            setPassword(e.target.value);
            setError(null);
          }}
          spellCheck={false}
          autoComplete="new-password"
        />
        <button
          type="button"
          className="act"
          title={t(show ? 'unlock.hidePassword' : 'unlock.showPassword')}
          aria-pressed={show}
          onClick={() => setShow((s) => !s)}
        >
          <Icon name={show ? 'eyeOff' : 'eye'} />
        </button>
      </div>
      <StrengthMeter password={password} />
      {!show && (
        <>
          <label className="unlock-label" htmlFor="newdb-again">
            {t('newdb.confirm')}
          </label>
          <div className="unlock-input">
            <input
              id="newdb-again"
              type="password"
              value={again}
              onChange={(e) => {
                setAgain(e.target.value);
                setError(null);
              }}
              aria-invalid={mismatch}
              spellCheck={false}
              autoComplete="new-password"
            />
          </div>
        </>
      )}

      <div className="unlock-label">{t('newdb.keyFile')}</div>
      {keyFile ? (
        <div className="unlock-keyfile" title={keyFile}>
          <Icon name="key" />
          <span className="unlock-keyfile-text">
            <span className="unlock-keyfile-label">{t('unlock.keyFileLabel')}</span>
            <span className="unlock-keyfile-name">{baseName(keyFile)}</span>
          </span>
          <button
            type="button"
            className="act"
            title={t('newdb.keyFileNone')}
            aria-label={t('newdb.keyFileNone')}
            onClick={() => setKeyFile(null)}
          >
            <Icon name="close" size={14} />
          </button>
        </div>
      ) : (
        <div className="newdb-keyfile">
          <button type="button" className="link-btn" onClick={() => void chooseKeyFile()}>
            <Icon name="folderOpen" />
            {t('newdb.keyFileChoose')}
          </button>
          <button type="button" className="link-btn" onClick={() => void makeKeyFile()}>
            <Icon name="key" />
            {t('newdb.keyFileCreate')}
          </button>
        </div>
      )}
      {keyFile && <p className="unlock-hint small">{t('newdb.keyFileHint')}</p>}

      <div className="unlock-msg" role="alert">
        {(error || mismatch) && (
          <span className="error">
            <Icon name="warn" />
            {t(error ?? 'newdb.mismatch')}
          </span>
        )}
      </div>
      <button type="submit" className="btn primary unlock-submit" disabled={busy}>
        {busy ? t('newdb.creating') : t('newdb.create')}
      </button>
      <button type="button" className="link-btn back" onClick={onBack} disabled={busy}>
        {t('newdb.back')}
      </button>
    </form>
  );
}
