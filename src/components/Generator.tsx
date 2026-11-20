import { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import { AppError, DEFAULT_GENERATOR, type Generated, type GeneratorOptions } from '../api/types';
import { usePrefs, useToast } from '../app/contexts';
import { Icon } from './Icon';
import { StrengthBars } from './Strength';

interface Props {
  /** "Use this password": fills the editor's password field */
  onUse?: (password: string) => void;
  /** standalone (Ctrl+G): offer copying instead */
  standalone?: boolean;
}

/**
 * The password generator's options and its result. The password is made on
 * the Rust side (OS random source); the last options are remembered there.
 */
export function GeneratorPanel({ onUse, standalone }: Props) {
  const { t, settings } = usePrefs();
  const toast = useToast();
  const [opts, setOpts] = useState<GeneratorOptions>(() => ({
    ...DEFAULT_GENERATOR,
    ...settings.generator,
  }));
  const [result, setResult] = useState<Generated | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = useCallback(
    (o: GeneratorOptions) =>
      api.generatePassword(o).then(
        (g) => {
          setResult(g);
          setError(null);
        },
        (e: unknown) => {
          setResult(null);
          setError(t(`error.${e instanceof AppError ? e.code : 'internal'}`));
        },
      ),
    [t],
  );

  // a new password whenever an option changes
  useEffect(() => {
    void run(opts);
  }, [opts, run]);

  const set = <K extends keyof GeneratorOptions>(key: K, value: GeneratorOptions[K]) =>
    setOpts((o) => ({ ...o, [key]: value }));

  const check = (
    key: 'lower' | 'upper' | 'digits' | 'symbols' | 'excludeLookAlike' | 'capitalize' | 'addDigit',
    label: string,
  ) => (
    <label className="check">
      <input type="checkbox" checked={opts[key]} onChange={(e) => set(key, e.target.checked)} />
      <span>{label}</span>
    </label>
  );

  const copy = () => {
    if (!result) return;
    api.copySecret(result.password).then(
      (r) =>
        toast(
          r.clearsAfter
            ? {
                message: t('toast.copiedClears', { field: t('field.password'), n: r.clearsAfter }),
                clearsAfter: r.clearsAfter,
              }
            : { message: t('toast.copied', { field: t('field.password') }) },
        ),
      (e: unknown) =>
        toast({ message: t(`error.${e instanceof AppError ? e.code : 'internal'}`), tone: 'error' }),
    );
  };

  return (
    <div className="generator">
      <div className="gen-out">
        <output className="gen-password" aria-live="polite">
          {result?.password ?? error ?? '…'}
        </output>
        <button
          type="button"
          className="act"
          title={t('gen.again')}
          aria-label={t('gen.again')}
          onClick={() => void run(opts)}
        >
          <Icon name="restore" />
        </button>
      </div>
      <div className="gen-meta">
        <StrengthBars strength={result?.strength ?? null} />
        {result && <span className="gen-bits">{t('gen.bits', { n: Math.round(result.bits) })}</span>}
      </div>

      <div className="seg" role="radiogroup">
        {(['password', 'passphrase'] as const).map((m) => (
          <button
            key={m}
            type="button"
            role="radio"
            aria-checked={opts.mode === m}
            className={opts.mode === m ? 'on' : ''}
            onClick={() => set('mode', m)}
          >
            {t(m === 'password' ? 'gen.password' : 'gen.passphrase')}
          </button>
        ))}
      </div>

      {opts.mode === 'password' ? (
        <div className="gen-opts">
          <label className="gen-range">
            <span>{t('gen.length')}</span>
            <input
              type="range"
              min={4}
              max={64}
              value={Math.min(opts.length, 64)}
              onChange={(e) => set('length', Number(e.target.value))}
            />
            <input
              type="number"
              min={4}
              max={128}
              value={opts.length}
              onChange={(e) => set('length', Number(e.target.value) || 4)}
            />
          </label>
          <div className="gen-checks">
            {check('lower', t('gen.lower'))}
            {check('upper', t('gen.upper'))}
            {check('digits', t('gen.digits'))}
            {check('symbols', t('gen.symbols'))}
            {check('excludeLookAlike', t('gen.lookAlike'))}
          </div>
        </div>
      ) : (
        <div className="gen-opts">
          <label className="gen-range">
            <span>{t('gen.words')}</span>
            <input
              type="range"
              min={3}
              max={12}
              value={opts.words}
              onChange={(e) => set('words', Number(e.target.value))}
            />
            <span className="gen-num">{opts.words}</span>
          </label>
          <label className="gen-sep">
            <span>{t('gen.separator')}</span>
            <input
              value={opts.separator}
              maxLength={3}
              onChange={(e) => set('separator', e.target.value)}
              spellCheck={false}
            />
          </label>
          <div className="gen-checks">
            {check('capitalize', t('gen.capitalize'))}
            {check('addDigit', t('gen.addDigit'))}
          </div>
          <div className="gen-note">{t('gen.wordlistNote')}</div>
        </div>
      )}

      <div className="gen-actions">
        {standalone ? (
          <button type="button" className="btn primary" disabled={!result} onClick={copy}>
            <Icon name="copy" />
            {t('gen.copy')}
          </button>
        ) : (
          <button
            type="button"
            className="btn primary"
            disabled={!result}
            onClick={() => result && onUse?.(result.password)}
          >
            {t('gen.use')}
          </button>
        )}
      </div>
    </div>
  );
}
