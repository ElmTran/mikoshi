import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { api } from '../api';
import { AppError, type HotkeyState } from '../api/types';
import { usePrefs } from '../app/contexts';
import { Keys } from './bits';
import { Icon } from './Icon';
import { hotkeyFromEvent, usableHotkey } from '../hotkey';

interface Props {
  label: string;
  value: string;
  /** what registering it with Windows came to (global hotkeys only) */
  state?: HotkeyState | undefined;
  /** shows "Restore default" while the value differs from it */
  defaultValue?: string | undefined;
  /** a problem with the current value, e.g. used twice */
  note?: string | null | undefined;
  /** why `text` cannot be used, or null when it can; global hotkeys by default */
  validate?: ((text: string) => string | null) | undefined;
  onSave: (text: string) => Promise<void>;
}

/**
 * One hotkey or shortcut in the settings. Click, then press the keys; Esc
 * cancels, Backspace turns it off. The global hotkeys are paused while
 * recording, so the current ones can be pressed too.
 */
export function HotkeyField({ label, value, state, defaultValue, note, validate, onSave }: Props) {
  const { t } = usePrefs();
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const recordingRef = useRef(false);
  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);

  // never leave the hotkeys paused, e.g. when the settings close mid-recording
  useEffect(
    () => () => {
      if (recordingRef.current) void api.pauseHotkeys(false).catch(() => {});
    },
    [],
  );

  const check = validate ?? ((text: string) => (usableHotkey(text) ? null : t('error.hotkeyInvalid')));

  const start = () => {
    setError(null);
    setRecording(true);
    void api.pauseHotkeys(true).catch(() => {});
  };
  const cancel = () => {
    setRecording(false);
    void api.pauseHotkeys(false).catch(() => {});
  };
  const save = (text: string) => {
    setRecording(false);
    // saving registers the hotkeys again, also when it fails
    onSave(text).then(
      () => setError(null),
      (e: unknown) => setError(t(`error.${e instanceof AppError ? e.code : 'internal'}`)),
    );
    void api.pauseHotkeys(false).catch(() => {});
  };

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    const plain = !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
    if (plain && e.key === 'Escape') return cancel();
    if (plain && e.key === 'Backspace') return save('');
    const text = hotkeyFromEvent(e);
    if (!text) return;
    const problem = check(text);
    if (problem) {
      setError(problem);
      return;
    }
    save(text);
  };

  const warning =
    error ??
    note ??
    (state === 'taken'
      ? t('settings.hotkeyTaken')
      : state === 'invalid'
        ? t('settings.hotkeyInvalid')
        : null);

  return (
    <div className="hotkey-row">
      <div className="select-row">
        <span>{label}</span>
        <span className="hotkey-controls">
          {defaultValue !== undefined && value !== defaultValue && !recording && (
            <button
              type="button"
              className="link-btn small"
              title={defaultValue ? defaultValue : t('settings.hotkeyOff')}
              onClick={() => save(defaultValue)}
            >
              {t('settings.restoreDefault')}
            </button>
          )}
          <button
            type="button"
            className={recording ? 'hotkey-field recording' : 'hotkey-field'}
            title={t('settings.hotkeyHint')}
            aria-label={`${label}: ${recording ? t('settings.hotkeyRecording') : value || t('settings.hotkeyOff')}`}
            onClick={() => (recording ? cancel() : start())}
            onKeyDown={onKeyDown}
            onBlur={() => recording && cancel()}
            {...(recording ? { 'data-own-escape': '' } : {})}
          >
            {recording ? (
              <span className="hotkey-placeholder">{t('settings.hotkeyRecording')}</span>
            ) : value ? (
              <Keys keys={value.split('+')} />
            ) : (
              <span className="hotkey-placeholder">{t('settings.hotkeyOff')}</span>
            )}
          </button>
        </span>
      </div>
      {warning && (
        <div className="hotkey-warning" role="alert">
          <Icon name="warn" size={12} />
          {warning}
        </div>
      )}
    </div>
  );
}
