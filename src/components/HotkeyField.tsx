import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { api } from '../api';
import { AppError, type HotkeyState } from '../api/types';
import { usePrefs } from '../app/contexts';
import { Keys } from './bits';
import { Icon } from './Icon';
import { hotkeyFromEvent, usableHotkey } from '../hotkey';

/**
 * One global hotkey in the settings. Click, then press the keys; Esc cancels,
 * Backspace or Delete turns the hotkey off. The global hotkeys are paused
 * while recording, so the current ones can be pressed too.
 */
export function HotkeyField({
  label,
  value,
  state,
  onSave,
}: {
  label: string;
  value: string;
  state: HotkeyState | undefined;
  onSave: (text: string) => Promise<void>;
}) {
  const { t } = usePrefs();
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const recordingRef = useRef(false);
  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);

  // never leave the hotkeys paused, e.g. when the flyout closes mid-recording
  useEffect(
    () => () => {
      if (recordingRef.current) void api.pauseHotkeys(false).catch(() => {});
    },
    [],
  );

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
  };

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    const plain = !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
    if (plain && e.key === 'Escape') return cancel();
    if (plain && (e.key === 'Backspace' || e.key === 'Delete')) return save('');
    const text = hotkeyFromEvent(e);
    if (!text) return;
    if (!usableHotkey(text)) {
      setError(t('error.hotkeyInvalid'));
      return;
    }
    save(text);
  };

  const warning =
    error ??
    (state === 'taken'
      ? t('settings.hotkeyTaken')
      : state === 'invalid'
        ? t('settings.hotkeyInvalid')
        : null);

  return (
    <div className="hotkey-row">
      <div className="select-row">
        <span>{label}</span>
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
