import { useCallback, useEffect, useRef, useState, type MouseEvent } from 'react';
import { api, listen } from '../api';
import { AppError, type HotkeyStatus, type LanguagePref, type ThemePref } from '../api/types';
import { usePrefs, useToast } from '../app/contexts';
import { HotkeyField } from './HotkeyField';
import { Icon, type IconName } from './Icon';
import { useDismiss } from './useDismiss';

const THEMES: { value: ThemePref; icon: IconName }[] = [
  { value: 'system', icon: 'monitor' },
  { value: 'light', icon: 'sun' },
  { value: 'dark', icon: 'moon' },
];
const LANGUAGES: LanguagePref[] = ['system', 'zh-CN', 'en-US'];
// 0 means never
const IDLE_MINUTES = [1, 2, 5, 10, 15, 30, 60, 0];
const CLEAR_SECONDS = [5, 10, 20, 30, 60, 0];
const KEY_DELAYS = [5, 10, 20, 50, 100];

/** The offered choices, plus the current value if it is not one of them. */
const withCurrent = (choices: number[], current: number) =>
  choices.includes(current) ? choices : [...choices, current].sort((a, b) => (a || 1e9) - (b || 1e9));

function Switch({
  on,
  label,
  hint,
  onToggle,
}: {
  on: boolean;
  label: string;
  hint: string;
  onToggle: () => void;
}) {
  return (
    <button type="button" role="switch" aria-checked={on} className="switch-row" onClick={onToggle}>
      <span className="switch-text">
        <span>{label}</span>
        <span className="switch-hint">{hint}</span>
      </span>
      <span className={on ? 'switch on' : 'switch'} aria-hidden="true" />
    </button>
  );
}

/** Small flyout under the title bar: appearance, language, security, system. */
export function SettingsButton() {
  const { t, tn, settings, update, info } = usePrefs();
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, root, close);

  // what the hotkeys and start-with-Windows are really doing, read on opening
  const [hotkeys, setHotkeys] = useState<HotkeyStatus | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  useEffect(() => {
    if (!open) return;
    api.hotkeyStatus().then(setHotkeys, () => {});
    api.getAutostart().then(setAutostart, () => setAutostart(null));
    const off = listen<HotkeyStatus>('hotkeys-changed', setHotkeys);
    return () => void off.then((f) => f());
  }, [open]);
  const toggleAutostart = () => {
    if (autostart === null) return;
    api
      .setAutostart(!autostart)
      .then(setAutostart, (e: unknown) =>
        toast({ message: t(`error.${e instanceof AppError ? e.code : 'internal'}`), tone: 'error' }),
      );
  };

  const pickTheme = (value: ThemePref, e: MouseEvent<HTMLButtonElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    void update({ theme: value }, { x: r.left + r.width / 2, y: r.top + r.height / 2 });
  };

  return (
    <div className="settings-anchor" ref={root}>
      <button
        className={open ? 'tb-btn icon-only active' : 'tb-btn icon-only'}
        title={`${t('toolbar.settings')} (Ctrl+,)`}
        aria-label={t('toolbar.settings')}
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        data-settings-toggle
      >
        <Icon name="settings" />
      </button>
      {open && (
        <div className="flyout settings-flyout" role="dialog" aria-label={t('settings.title')}>
          <div className="flyout-section">
            <div className="flyout-label">{t('settings.theme')}</div>
            <div className="segmented" role="radiogroup" aria-label={t('settings.theme')}>
              {THEMES.map((o) => (
                <button
                  key={o.value}
                  role="radio"
                  aria-checked={settings.theme === o.value}
                  className={settings.theme === o.value ? 'seg on' : 'seg'}
                  onClick={(e) => pickTheme(o.value, e)}
                >
                  <Icon name={o.icon} />
                  <span>{t(`settings.theme.${o.value}`)}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="flyout-section">
            <div className="flyout-label">{t('settings.language')}</div>
            <div className="segmented" role="radiogroup" aria-label={t('settings.language')}>
              {LANGUAGES.map((l) => (
                <button
                  key={l}
                  role="radio"
                  aria-checked={settings.language === l}
                  className={settings.language === l ? 'seg on' : 'seg'}
                  onClick={() => void update({ language: l })}
                >
                  {l === 'system' && <Icon name="globe" />}
                  <span>{t(`settings.language.${l}`)}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="flyout-section">
            <div className="flyout-label">{t('settings.security')}</div>
            <label className="select-row">
              <span>{t('settings.idleLock')}</span>
              <select
                className="select"
                value={settings.idleLockMinutes}
                onChange={(e) => void update({ idleLockMinutes: Number(e.target.value) })}
              >
                {withCurrent(IDLE_MINUTES, settings.idleLockMinutes).map((m) => (
                  <option key={m} value={m}>
                    {m === 0 ? t('settings.never') : tn('settings.minutes', m)}
                  </option>
                ))}
              </select>
            </label>
            <label className="select-row">
              <span>{t('settings.clipboardClear')}</span>
              <select
                className="select"
                value={settings.clipboardClearSeconds}
                onChange={(e) => void update({ clipboardClearSeconds: Number(e.target.value) })}
              >
                {withCurrent(CLEAR_SECONDS, settings.clipboardClearSeconds).map((s) => (
                  <option key={s} value={s}>
                    {s === 0 ? t('settings.never') : tn('settings.seconds', s)}
                  </option>
                ))}
              </select>
            </label>
            <Switch
              on={settings.lockOnMinimize}
              label={t('settings.lockOnMinimize')}
              hint={t('settings.lockOnMinimizeHint')}
              onToggle={() => void update({ lockOnMinimize: !settings.lockOnMinimize })}
            />
            <label className="select-row" title={t('settings.lockUnsavedHint')}>
              <span>{t('settings.lockUnsaved')}</span>
              <select
                className="select"
                value={settings.lockUnsaved}
                onChange={(e) => void update({ lockUnsaved: e.target.value === 'ask' ? 'ask' : 'save' })}
              >
                <option value="save">{t('settings.lockUnsaved.save')}</option>
                <option value="ask">{t('settings.lockUnsaved.ask')}</option>
              </select>
            </label>
            <Switch
              on={settings.backupOnSave}
              label={t('settings.backupOnSave')}
              hint={t('settings.backupOnSaveHint')}
              onToggle={() => void update({ backupOnSave: !settings.backupOnSave })}
            />
            <Switch
              on={settings.rememberKeyFiles}
              label={t('settings.rememberKeyFiles')}
              hint={t('settings.rememberKeyFilesHint')}
              onToggle={() => void update({ rememberKeyFiles: !settings.rememberKeyFiles })}
            />
          </div>
          <div className="flyout-section">
            <div className="flyout-label">{t('settings.system')}</div>
            <HotkeyField
              label={t('settings.quickSearchHotkey')}
              value={settings.quickSearchHotkey}
              state={hotkeys?.quickSearch}
              onSave={(text) => update({ quickSearchHotkey: text })}
            />
            <HotkeyField
              label={t('settings.lockHotkey')}
              value={settings.lockHotkey}
              state={hotkeys?.lock}
              onSave={(text) => update({ lockHotkey: text })}
            />
            <HotkeyField
              label={t('settings.autotypeHotkey')}
              value={settings.autotypeHotkey}
              state={hotkeys?.autotype}
              onSave={(text) => update({ autotypeHotkey: text })}
            />
            <label className="select-row">
              <span>{t('settings.keyDelay')}</span>
              <select
                className="select"
                value={settings.autotypeKeyDelayMs}
                onChange={(e) => void update({ autotypeKeyDelayMs: Number(e.target.value) })}
              >
                {withCurrent(KEY_DELAYS, settings.autotypeKeyDelayMs).map((ms) => (
                  <option key={ms} value={ms}>
                    {t('settings.milliseconds', { n: ms })}
                  </option>
                ))}
              </select>
            </label>
            <Switch
              on={autostart === true}
              label={t('settings.autostart')}
              hint={t('settings.autostartHint')}
              onToggle={toggleAutostart}
            />
          </div>
          {info && <div className="flyout-foot">{t('settings.version', { v: info.version })}</div>}
        </div>
      )}
    </div>
  );
}
