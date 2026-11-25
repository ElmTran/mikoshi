import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { api, listen } from '../api';
import {
  AppError,
  type CipherName,
  type DatabaseSettings,
  type DialogText,
  type Encryption,
  type KdfName,
  type KeyInput,
  type SaveOptions,
  type VaultStatus,
} from '../api/types';
import { usePrefs, useToast } from '../app/contexts';
import type { MessageKey } from '../i18n/core';
import { baseName } from '../text';
import { Dialog } from './Dialog';
import { Icon } from './Icon';
import { Section, Switch } from './settingsBits';
import { StrengthMeter } from './Strength';

const MIB = 1 << 20;
const MEMORY_MIB = [16, 32, 64, 128, 256, 512, 1024, 2048];
const LANES = [1, 2, 4, 8, 16];
/** above this, other devices (phones) may struggle; mirrors ARGON2_MEMORY_HEAVY */
const HEAVY_MEMORY = 256 * MIB;
const SLOW_MS = 5000;
const CIPHERS: CipherName[] = ['aes256', 'chaCha20', 'twofish'];
const KDFS: KdfName[] = ['argon2id', 'argon2d', 'aesKdf'];

const errorKey = (e: unknown): MessageKey => `error.${e instanceof AppError ? e.code : 'internal'}`;
const codeOf = (e: unknown) => (e instanceof AppError ? e.code : 'internal');

/** The same settings, ignoring what the KDF does not use. */
const sameEncryption = (a: Encryption, b: Encryption) =>
  a.cipher === b.cipher &&
  a.kdf === b.kdf &&
  a.iterations === b.iterations &&
  a.compressed === b.compressed &&
  (a.kdf === 'aesKdf' || (a.memory === b.memory && a.parallelism === b.parallelism));

const withCurrent = (choices: number[], current: number) =>
  choices.includes(current) ? choices : [...choices, current].sort((a, b) => a - b);

/** A save that needs the user's answer first, and the options to retry with. */
interface Ask {
  kind: 'conflict' | 'upgrade';
  options: SaveOptions;
}

/**
 * Runs `save` with `options`; a conflict or the 4.1 question becomes an
 * `Ask` (answered by `SaveQuestion`), other errors are returned as is.
 */
async function trySave<T>(
  save: (options: SaveOptions) => Promise<T>,
  options: SaveOptions,
  onAsk: (a: Ask) => void,
): Promise<T | null> {
  try {
    return await save(options);
  } catch (e) {
    const code = codeOf(e);
    if (code === 'saveConflict') onAsk({ kind: 'conflict', options });
    else if (code === 'formatUpgrade') onAsk({ kind: 'upgrade', options });
    else throw e;
    return null;
  }
}

/** The conflict and 4.1 questions, for a save started on the settings page. */
function SaveQuestion({
  ask,
  onAnswer,
  onCancel,
}: {
  ask: Ask;
  onAnswer: (options: SaveOptions) => void;
  onCancel: () => void;
}) {
  const { t } = usePrefs();
  return ask.kind === 'upgrade' ? (
    <Dialog
      title={t('dialog.upgrade.title')}
      onClose={onCancel}
      actions={[
        { label: t('dialog.conflict.cancel'), onClick: onCancel },
        {
          label: t('dialog.upgrade.save'),
          primary: true,
          onClick: () => onAnswer({ ...ask.options, upgrade: true }),
        },
      ]}
    >
      <p>{t('dialog.upgrade.message')}</p>
    </Dialog>
  ) : (
    <Dialog
      title={t('dialog.conflict.title')}
      onClose={onCancel}
      actions={[
        { label: t('dialog.conflict.cancel'), primary: true, onClick: onCancel },
        {
          label: t('dialog.conflict.overwrite'),
          danger: true,
          onClick: () => onAnswer({ ...ask.options, force: true }),
        },
      ]}
    >
      <p>{t('dialog.conflict.message')}</p>
    </Dialog>
  );
}

/** A pill row of choices, like the theme picker. */
function Choice<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented small" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          className={value === o.value ? 'seg on' : 'seg'}
          onClick={() => onChange(o.value)}
        >
          <span>{o.label}</span>
        </button>
      ))}
    </div>
  );
}

type PasswordMode = 'keep' | 'set' | 'remove';
type KeyFileMode = 'keep' | 'set' | 'remove';

/**
 * Changing the master key: the current password (or key file, for a
 * database without a password) first, then what the new key is made of.
 * Saves at once. Afterwards, when a backup of the old file lies next to the
 * database, offers to delete it: the old key still opens it.
 */
function ChangeKeyDialog({
  info,
  path,
  onClose,
  onChanged,
}: {
  info: DatabaseSettings;
  path: string;
  onClose: () => void;
  onChanged: (s: DatabaseSettings) => void;
}) {
  const { t } = usePrefs();
  const toast = useToast();
  const [current, setCurrent] = useState('');
  const [currentKeyFile, setCurrentKeyFile] = useState<string | null>(null);
  const [pwMode, setPwMode] = useState<PasswordMode>(info.hasPassword ? 'set' : 'keep');
  const [password, setPassword] = useState('');
  const [again, setAgain] = useState('');
  const [show, setShow] = useState(false);
  const [kfMode, setKfMode] = useState<KeyFileMode>('keep');
  const [keyFile, setKeyFile] = useState<string | null>(null);
  const [error, setError] = useState<MessageKey | null>(null);
  const [busy, setBusy] = useState(false);
  const [ask, setAsk] = useState<Ask | null>(null);
  const [backupStep, setBackupStep] = useState(false);
  const first = useRef<HTMLInputElement>(null);

  // after the dialog has focused its primary button: start in the first field
  useEffect(() => {
    const id = window.setTimeout(() => first.current?.focus(), 0);
    return () => window.clearTimeout(id);
  }, []);

  const dialog = (title: string): DialogText => ({
    title,
    filter: t('dialog.keyFileFilter'),
    allFiles: t('dialog.allFiles'),
  });
  const pick = (set: (p: string) => void) =>
    api.chooseKeyFile(dialog(t('dialog.keyFile'))).then(
      (p) => p && set(p),
      (e: unknown) => setError(errorKey(e)),
    );
  const make = () =>
    api.createKeyFile(dialog(t('dialog.newKeyFile')), `${baseName(path).replace(/\.kdbx$/i, '')}.keyx`).then(
      (p) => p && setKeyFile(p),
      (e: unknown) => setError(errorKey(e)),
    );

  const willHavePassword = pwMode === 'set' ? password !== '' : pwMode === 'keep' && info.hasPassword;
  const willHaveKeyFile = kfMode === 'set' ? keyFile !== null : kfMode === 'keep' && info.hasKeyFile;
  const nothing = pwMode === 'keep' && kfMode === 'keep';
  const mismatch = pwMode === 'set' && !show && again !== '' && password !== again;

  const submit = (options: SaveOptions = {}) => {
    if (busy) return;
    if (nothing) return setError('dbs.dlg.nothing');
    if (info.hasPassword ? current === '' : currentKeyFile === null) {
      return setError('error.currentKeyWrong');
    }
    if (pwMode === 'set' && !show && password !== again) return setError('newdb.mismatch');
    if (kfMode === 'set' && keyFile === null) return setError('dbs.dlg.needKeyFile');
    if (!willHavePassword && !willHaveKeyFile) return setError('newdb.needKey');
    const key: KeyInput = {
      password:
        pwMode === 'set' && password !== ''
          ? { kind: 'set', value: password }
          : pwMode === 'keep'
            ? { kind: 'keep' }
            : { kind: 'remove' },
      keyFile:
        kfMode === 'set' && keyFile
          ? { kind: 'set', path: keyFile }
          : { kind: kfMode === 'keep' ? 'keep' : 'remove' },
    };
    setError(null);
    setAsk(null);
    setBusy(true);
    trySave(
      (o) =>
        api.applyDatabaseSettings({
          currentPassword: info.hasPassword ? current : null,
          currentKeyFile: info.hasPassword ? null : currentKeyFile,
          key,
          options: o,
        }),
      options,
      setAsk,
    ).then(
      (s) => {
        setBusy(false);
        if (!s) return;
        // nothing secret stays in the form
        setCurrent('');
        setPassword('');
        setAgain('');
        onChanged(s);
        if (s.hasBackup) setBackupStep(true);
        else {
          toast({ message: t('dbs.key.changed') });
          onClose();
        }
      },
      (e: unknown) => {
        setBusy(false);
        setError(errorKey(e));
      },
    );
  };

  if (ask) {
    return <SaveQuestion ask={ask} onAnswer={(o) => submit(o)} onCancel={() => setAsk(null)} />;
  }

  if (backupStep) {
    const bak = `${baseName(path)}.bak`;
    return (
      <Dialog
        title={t('dbs.bak.title')}
        onClose={onClose}
        actions={[
          { label: t('dbs.bak.keep'), onClick: onClose },
          {
            label: t('dbs.bak.delete'),
            primary: true,
            danger: true,
            onClick: () =>
              api.removeBackup().then(
                () => {
                  toast({ message: t('dbs.backup.deleted') });
                  void api.getDatabaseSettings().then(onChanged, () => {});
                  onClose();
                },
                (e: unknown) => toast({ message: t(errorKey(e)), tone: 'error' }),
              ),
          },
        ]}
      >
        <p>{t('dbs.bak.message', { file: bak })}</p>
      </Dialog>
    );
  }

  const pwOptions: { value: PasswordMode; label: string }[] = info.hasPassword
    ? [
        { value: 'set', label: t('dbs.dlg.pw.set') },
        { value: 'keep', label: t('dbs.dlg.pw.keep') },
        { value: 'remove', label: t('dbs.dlg.pw.none') },
      ]
    : [
        { value: 'keep', label: t('dbs.dlg.pw.none') },
        { value: 'set', label: t('dbs.dlg.pw.add') },
      ];
  const kfOptions: { value: KeyFileMode; label: string }[] = info.hasKeyFile
    ? [
        { value: 'keep', label: t('dbs.dlg.kf.keep') },
        { value: 'set', label: t('dbs.dlg.kf.set') },
        { value: 'remove', label: t('dbs.dlg.kf.remove') },
      ]
    : [
        { value: 'keep', label: t('dbs.dlg.kf.none') },
        { value: 'set', label: t('dbs.dlg.kf.add') },
      ];

  return (
    <Dialog
      title={t('dbs.dlg.title')}
      className="change-key"
      onClose={() => !busy && onClose()}
      actions={[
        { label: t('dbs.dlg.cancel'), onClick: () => !busy && onClose() },
        { label: busy ? t('dbs.dlg.saving') : t('dbs.dlg.submit'), primary: true, onClick: () => submit() },
      ]}
    >
      <form
        className="change-key-form"
        autoComplete="off"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        {info.hasPassword ? (
          <>
            <label className="unlock-label" htmlFor="ck-current">
              {t('dbs.dlg.current')}
            </label>
            <div className="unlock-input">
              <input
                id="ck-current"
                ref={first}
                type="password"
                value={current}
                onChange={(e) => {
                  setCurrent(e.target.value);
                  setError(null);
                }}
                spellCheck={false}
                autoComplete="current-password"
              />
            </div>
          </>
        ) : (
          <>
            <div className="unlock-label">{t('dbs.dlg.currentKeyFile')}</div>
            <KeyFileRow
              path={currentKeyFile}
              onChoose={() => void pick(setCurrentKeyFile)}
              chooseLabel={t('dbs.dlg.chooseCurrent')}
            />
          </>
        )}

        <div className="unlock-label">{t('dbs.dlg.password')}</div>
        <Choice
          label={t('dbs.dlg.password')}
          value={pwMode}
          options={pwOptions}
          onChange={(m) => {
            setPwMode(m);
            setError(null);
          }}
        />
        {pwMode === 'set' && (
          <>
            <div className="unlock-input">
              <input
                aria-label={t('dbs.dlg.newPassword')}
                placeholder={t('dbs.dlg.newPassword')}
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
              <div className="unlock-input">
                <input
                  aria-label={t('newdb.confirm')}
                  placeholder={t('newdb.confirm')}
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
            )}
          </>
        )}

        <div className="unlock-label">{t('dbs.dlg.keyFile')}</div>
        <Choice
          label={t('dbs.dlg.keyFile')}
          value={kfMode}
          options={kfOptions}
          onChange={(m) => {
            setKfMode(m);
            setError(null);
          }}
        />
        {kfMode === 'set' && (
          <>
            <KeyFileRow
              path={keyFile}
              onChoose={() => void pick(setKeyFile)}
              onMake={() => void make()}
              onClear={() => setKeyFile(null)}
              chooseLabel={t('newdb.keyFileChoose')}
            />
            <p className="unlock-hint small">{t('newdb.keyFileHint')}</p>
          </>
        )}

        <p className="dialog-hint">{t('dbs.dlg.otherDevices')}</p>
        <div className="unlock-msg" role="alert">
          {(error || mismatch) && (
            <span className="error">
              <Icon name="warn" />
              {t(error ?? 'newdb.mismatch')}
            </span>
          )}
        </div>
      </form>
    </Dialog>
  );
}

/** A chosen key file, or the buttons to choose (or make) one. */
function KeyFileRow({
  path,
  onChoose,
  onMake,
  onClear,
  chooseLabel,
}: {
  path: string | null;
  onChoose: () => void;
  onMake?: () => void;
  onClear?: () => void;
  chooseLabel: string;
}) {
  const { t } = usePrefs();
  if (path) {
    return (
      <div className="unlock-keyfile" title={path}>
        <Icon name="key" />
        <span className="unlock-keyfile-text">
          <span className="unlock-keyfile-label">{t('unlock.keyFileLabel')}</span>
          <span className="unlock-keyfile-name">{baseName(path)}</span>
        </span>
        <button
          type="button"
          className="act"
          title={t('newdb.keyFileNone')}
          aria-label={t('newdb.keyFileNone')}
          onClick={onClear ?? onChoose}
        >
          <Icon name={onClear ? 'close' : 'folderOpen'} size={14} />
        </button>
      </div>
    );
  }
  return (
    <div className="newdb-keyfile">
      <button type="button" className="link-btn" onClick={onChoose}>
        <Icon name="folderOpen" />
        {chooseLabel}
      </button>
      {onMake && (
        <button type="button" className="link-btn" onClick={onMake}>
          <Icon name="key" />
          {t('newdb.keyFileCreate')}
        </button>
      )}
    </div>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string | undefined; children: ReactNode }) {
  return (
    <div className="select-row with-hint">
      <span className="switch-text">
        <span>{label}</span>
        {hint && <span className="switch-hint">{hint}</span>}
      </span>
      <span className="row-controls">{children}</span>
    </div>
  );
}

/** Lanes for a new Argon2 setting: one per core, as for a new database. */
const defaultLanes = () => Math.min(16, Math.max(1, navigator.hardwareConcurrency || 2));

/**
 * The "Database" category of the settings: the open database's master key
 * and encryption settings. Shown only while a database is unlocked.
 */
export function DatabasePanel({ status }: { status: VaultStatus }) {
  const { t } = usePrefs();
  const toast = useToast();
  const [info, setInfo] = useState<DatabaseSettings | null>(null);
  const [draft, setDraft] = useState<Encryption | null>(null);
  const [loadError, setLoadError] = useState<MessageKey | null>(null);
  const [busy, setBusy] = useState<'apply' | 'measure' | 'calibrate' | null>(null);
  const [measured, setMeasured] = useState<number | null>(null);
  const [ask, setAsk] = useState<Ask | null>(null);
  const [changingKey, setChangingKey] = useState(false);

  // what the file holds; a draft that differs from it is kept when it is read again
  const saved = useRef<DatabaseSettings | null>(null);
  const take = useCallback((s: DatabaseSettings) => {
    const before = saved.current;
    saved.current = s;
    setInfo(s);
    setDraft((d) => (d && before && !sameEncryption(d, before.encryption) ? d : s.encryption));
  }, []);
  /** after a save of the encryption itself: the draft is what was saved */
  const takeSaved = useCallback((s: DatabaseSettings) => {
    saved.current = s;
    setInfo(s);
    setDraft(s.encryption);
  }, []);

  const load = useCallback(
    () =>
      api.getDatabaseSettings().then(
        (s) => {
          take(s);
          setLoadError(null);
        },
        (e: unknown) => setLoadError(errorKey(e)),
      ),
    [take],
  );
  useEffect(() => {
    void load();
    // a reload or a save elsewhere can change what the file holds
    const off = listen('vault-changed', () => void load());
    return () => void off.then((f) => f());
  }, [load]);

  if (loadError) return <p className="settings-note">{t(loadError)}</p>;
  if (!info || !draft) return null;

  const fail = (e: unknown) => toast({ message: t(errorKey(e)), tone: 'error' });
  const changed = !sameEncryption(draft, info.encryption);
  const isArgon = draft.kdf !== 'aesKdf';
  const edit = (patch: Partial<Encryption>) => {
    setDraft({ ...draft, ...patch });
    setMeasured(null);
  };

  const measure = (e: Encryption) => {
    setBusy('measure');
    api.timeKdf(e).then(
      (ms) => {
        setMeasured(ms);
        setBusy(null);
      },
      (err: unknown) => {
        setBusy(null);
        fail(err);
      },
    );
  };
  const calibrate = (e: Encryption) => {
    setBusy('calibrate');
    api.calibrateKdf(e).then(
      (tuned) => {
        setDraft(tuned);
        setBusy(null);
        measure(tuned);
      },
      (err: unknown) => {
        setBusy(null);
        fail(err);
      },
    );
  };
  // another kind of KDF has other units: start from sensible values, tuned here
  const pickKdf = (kdf: KdfName) => {
    if (kdf === draft.kdf) return;
    const next: Encryption =
      kdf === 'aesKdf'
        ? { ...draft, kdf, iterations: 10_000_000, memory: 0, parallelism: 0 }
        : {
            ...draft,
            kdf,
            memory: draft.kdf === 'aesKdf' ? 64 * MIB : draft.memory,
            parallelism: draft.kdf === 'aesKdf' ? defaultLanes() : draft.parallelism,
            iterations: draft.kdf === 'aesKdf' ? 10 : draft.iterations,
          };
    setDraft(next);
    setMeasured(null);
    calibrate(next);
  };

  const apply = (options: SaveOptions = {}) => {
    setBusy('apply');
    setAsk(null);
    trySave((o) => api.applyDatabaseSettings({ encryption: draft, options: o }), options, setAsk).then(
      (s) => {
        setBusy(null);
        if (!s) return;
        takeSaved(s);
        toast({ message: t('dbs.enc.applied') });
      },
      (e: unknown) => {
        setBusy(null);
        fail(e);
      },
    );
  };

  const keyNow =
    info.hasPassword && info.hasKeyFile
      ? 'dbs.key.both'
      : info.hasKeyFile
        ? 'dbs.key.keyFile'
        : 'dbs.key.password';
  const mib = (bytes: number) => {
    const n = bytes / MIB;
    return n >= 1024 && n % 1024 === 0
      ? t('dbs.gib', { n: n / 1024 })
      : t('dbs.mib', { n: Number(n.toFixed(1)) });
  };
  const seconds = (ms: number) => (ms / 1000).toFixed(ms < 10_000 ? 1 : 0);
  const working = busy !== null;

  return (
    <>
      <Section title={t('dbs.key.title')} hint={t('dbs.key.hint')}>
        <Row label={t('dbs.key.now')} hint={t(keyNow)}>
          <button type="button" className="btn" onClick={() => setChangingKey(true)} disabled={working}>
            {t('dbs.key.change')}
          </button>
        </Row>
        {info.hasBackup && status.file && (
          <Row label={t('dbs.backup.row')} hint={t('dbs.backup.rowHint', { file: `${status.file}.bak` })}>
            <button
              type="button"
              className="btn"
              onClick={() =>
                api.removeBackup().then(() => {
                  toast({ message: t('dbs.backup.deleted') });
                  take({ ...info, hasBackup: false });
                }, fail)
              }
            >
              {t('dbs.backup.delete')}
            </button>
          </Row>
        )}
      </Section>

      <Section title={t('dbs.enc.title')} hint={t('dbs.enc.hint', { v: info.version })}>
        <label className="select-row">
          <span>{t('dbs.enc.cipher')}</span>
          <select
            className="select"
            value={draft.cipher}
            disabled={working}
            onChange={(e) => edit({ cipher: e.target.value as CipherName })}
          >
            {CIPHERS.map((c) => (
              <option key={c} value={c}>
                {t(`dbs.cipher.${c}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="select-row">
          <span>{t('dbs.enc.kdf')}</span>
          <select
            className="select"
            value={draft.kdf}
            disabled={working}
            onChange={(e) => pickKdf(e.target.value as KdfName)}
          >
            {KDFS.map((k) => (
              <option key={k} value={k}>
                {t(`dbs.enc.kdf.${k}`)}
              </option>
            ))}
          </select>
        </label>
        {isArgon && (
          <label className="select-row">
            <span>{t('dbs.enc.memory')}</span>
            <select
              className="select"
              value={draft.memory}
              disabled={working}
              onChange={(e) => edit({ memory: Number(e.target.value) })}
            >
              {withCurrent(
                MEMORY_MIB.map((m) => m * MIB),
                draft.memory,
              ).map((b) => (
                <option key={b} value={b}>
                  {mib(b)}
                </option>
              ))}
            </select>
          </label>
        )}
        <label className="select-row">
          <span>{isArgon ? t('dbs.enc.iterations') : t('dbs.enc.rounds')}</span>
          <NumberInput
            value={draft.iterations}
            min={isArgon ? 1 : 1000}
            disabled={working}
            onChange={(n) => edit({ iterations: n })}
          />
        </label>
        {isArgon && (
          <label className="select-row">
            <span>{t('dbs.enc.parallelism')}</span>
            <select
              className="select"
              value={draft.parallelism}
              disabled={working}
              onChange={(e) => edit({ parallelism: Number(e.target.value) })}
            >
              {withCurrent(LANES, draft.parallelism).map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
        )}
        <Switch
          on={draft.compressed}
          label={t('dbs.enc.compressed')}
          hint={t('dbs.enc.compressedHint')}
          onToggle={() => !working && edit({ compressed: !draft.compressed })}
        />
        <Row label={t('dbs.enc.unlockTime')} hint={t('dbs.enc.unlockTimeHint')}>
          <span className="row-value">
            {busy === 'measure'
              ? t('dbs.enc.measuring')
              : busy === 'calibrate'
                ? t('dbs.enc.calibrating')
                : measured === null
                  ? t('dbs.enc.notMeasured')
                  : t('dbs.enc.seconds', { s: seconds(measured) })}
          </span>
          <button type="button" className="link-btn small" disabled={working} onClick={() => measure(draft)}>
            {t('dbs.enc.measure')}
          </button>
          <button
            type="button"
            className="link-btn small"
            disabled={working}
            onClick={() => calibrate(draft)}
          >
            {t('dbs.enc.calibrate')}
          </button>
        </Row>
        {(isArgon && draft.memory > HEAVY_MEMORY) || (measured !== null && measured > SLOW_MS) ? (
          <p className="settings-warn">
            <Icon name="warn" />
            <span>
              {[
                isArgon && draft.memory > HEAVY_MEMORY ? t('dbs.enc.heavy') : null,
                measured !== null && measured > SLOW_MS ? t('dbs.enc.slow') : null,
              ]
                .filter(Boolean)
                .join(' ')}
            </span>
          </p>
        ) : null}
        <div className="settings-actions">
          {changed && (
            <button
              type="button"
              className="btn"
              disabled={working}
              onClick={() => {
                setDraft(info.encryption);
                setMeasured(null);
              }}
            >
              {t('dbs.enc.reset')}
            </button>
          )}
          <button
            type="button"
            className="btn primary"
            disabled={!changed || working}
            onClick={() => apply()}
          >
            {busy === 'apply' ? t('dbs.enc.applying') : t('dbs.enc.apply')}
          </button>
        </div>
      </Section>

      {ask &&
        createPortal(
          <SaveQuestion ask={ask} onAnswer={(o) => apply(o)} onCancel={() => setAsk(null)} />,
          document.body,
        )}
      {changingKey &&
        status.path &&
        createPortal(
          <ChangeKeyDialog
            info={info}
            path={status.path}
            onClose={() => setChangingKey(false)}
            onChanged={take}
          />,
          document.body,
        )}
    </>
  );
}

/** A whole number field that only reports usable values. */
function NumberInput({
  value,
  min,
  disabled,
  onChange,
}: {
  value: number;
  min: number;
  disabled: boolean;
  onChange: (n: number) => void;
}) {
  const [text, setText] = useState(String(value));
  // follow changes from outside (calibrating, undo), adjusting while rendering
  const [shownValue, setShownValue] = useState(value);
  if (value !== shownValue) {
    setShownValue(value);
    setText(String(value));
  }
  return (
    <input
      className="select num-input"
      inputMode="numeric"
      value={text}
      disabled={disabled}
      onChange={(e) => {
        const clean = e.target.value.replace(/[^\d]/g, '');
        setText(clean);
        const n = Number(clean);
        if (clean !== '' && Number.isSafeInteger(n) && n >= min) onChange(n);
      }}
      onBlur={() => setText(String(value))}
    />
  );
}
