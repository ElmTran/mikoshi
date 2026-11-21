import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { api, listen } from '../api';
import {
  AppError,
  DEFAULT_SETTINGS,
  type HotkeyStatus,
  type LanguagePref,
  type ThemePref,
} from '../api/types';
import { usePrefs, useToast } from '../app/contexts';
import type { MessageKey } from '../i18n/core';
import { SHORTCUT_ACTIONS, shortcutConflicts, usableShortcut } from '../shortcuts';
import { HotkeyField } from './HotkeyField';
import { Icon, type IconName } from './Icon';

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

const CATEGORIES = ['appearance', 'security', 'saving', 'shortcuts', 'autotype', 'system', 'about'] as const;
type Category = (typeof CATEGORIES)[number];
const CATEGORY_ICONS: Record<Category, IconName> = {
  appearance: 'sun',
  security: 'lock',
  saving: 'download',
  shortcuts: 'keyboard',
  autotype: 'keyboard',
  system: 'monitor',
  about: 'globe',
};

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
  hint?: string;
  onToggle: () => void;
}) {
  return (
    <button type="button" role="switch" aria-checked={on} className="switch-row" onClick={onToggle}>
      <span className="switch-text">
        <span>{label}</span>
        {hint && <span className="switch-hint">{hint}</span>}
      </span>
      <span className={on ? 'switch on' : 'switch'} aria-hidden="true" />
    </button>
  );
}

function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string | undefined;
  children: ReactNode;
}) {
  return (
    <section className="settings-section">
      <h2 className="settings-section-title">{title}</h2>
      {hint && <p className="settings-section-hint">{hint}</p>}
      <div className="settings-rows">{children}</div>
    </section>
  );
}

/**
 * The settings, as a page over the window (the title bar stays). Categories
 * on the left; Esc, Ctrl+, or Back closes it. Every change is saved at once.
 */
function SettingsPage({ onClose }: { onClose: () => void }) {
  const { t, tn, settings, update, info } = usePrefs();
  const toast = useToast();
  const [cat, setCat] = useState<Category>('appearance');
  const root = useRef<HTMLDivElement>(null);

  // what the hotkeys and start-with-Windows are really doing
  const [hotkeys, setHotkeys] = useState<HotkeyStatus | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  useEffect(() => {
    api.hotkeyStatus().then(setHotkeys, () => {});
    api.getAutostart().then(setAutostart, () => setAutostart(null));
    const off = listen<HotkeyStatus>('hotkeys-changed', setHotkeys);
    return () => void off.then((f) => f());
  }, []);

  useEffect(() => root.current?.focus(), []);

  const fail = useCallback(
    (e: unknown) =>
      toast({ message: t(`error.${e instanceof AppError ? e.code : 'internal'}`), tone: 'error' }),
    [t, toast],
  );

  const toggleAutostart = () => {
    if (autostart === null) return;
    api.setAutostart(!autostart).then(setAutostart, fail);
  };
  const pickTheme = (value: ThemePref, e: MouseEvent<HTMLButtonElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    void update({ theme: value }, { x: r.left + r.width / 2, y: r.top + r.height / 2 });
  };

  const globals = useMemo(
    () =>
      [
        settings.quickSearchHotkey,
        settings.lockHotkey,
        settings.autotypeHotkey,
        settings.quickSaveHotkey,
      ].filter(Boolean),
    [settings.quickSearchHotkey, settings.lockHotkey, settings.autotypeHotkey, settings.quickSaveHotkey],
  );
  const conflicts = useMemo(
    () => shortcutConflicts(settings.shortcuts, globals),
    [settings.shortcuts, globals],
  );
  const shortcutsChanged = SHORTCUT_ACTIONS.some(
    (a) => settings.shortcuts[a] !== DEFAULT_SETTINGS.shortcuts[a],
  );

  const select = (
    label: string,
    value: number,
    choices: number[],
    show: (n: number) => string,
    set: (n: number) => void,
  ) => (
    <label className="select-row">
      <span>{label}</span>
      <select className="select" value={value} onChange={(e) => set(Number(e.target.value))}>
        {withCurrent(choices, value).map((n) => (
          <option key={n} value={n}>
            {show(n)}
          </option>
        ))}
      </select>
    </label>
  );

  const content: Record<Category, () => ReactNode> = {
    appearance: () => (
      <>
        <Section title={t('settings.theme')}>
          <div className="segmented" role="radiogroup" aria-label={t('settings.theme')}>
            {THEMES.map((o) => (
              <button
                key={o.value}
                type="button"
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
        </Section>
        <Section title={t('settings.language')}>
          <div className="segmented" role="radiogroup" aria-label={t('settings.language')}>
            {LANGUAGES.map((l) => (
              <button
                key={l}
                type="button"
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
        </Section>
      </>
    ),
    security: () => (
      <Section title={t('settings.cat.security')}>
        {select(
          t('settings.idleLock'),
          settings.idleLockMinutes,
          IDLE_MINUTES,
          (m) => (m === 0 ? t('settings.never') : tn('settings.minutes', m)),
          (n) => void update({ idleLockMinutes: n }),
        )}
        {select(
          t('settings.clipboardClear'),
          settings.clipboardClearSeconds,
          CLEAR_SECONDS,
          (s) => (s === 0 ? t('settings.never') : tn('settings.seconds', s)),
          (n) => void update({ clipboardClearSeconds: n }),
        )}
        <Switch
          on={settings.lockOnMinimize}
          label={t('settings.lockOnMinimize')}
          hint={t('settings.lockOnMinimizeHint')}
          onToggle={() => void update({ lockOnMinimize: !settings.lockOnMinimize })}
        />
        <Switch
          on={settings.rememberKeyFiles}
          label={t('settings.rememberKeyFiles')}
          hint={t('settings.rememberKeyFilesHint')}
          onToggle={() => void update({ rememberKeyFiles: !settings.rememberKeyFiles })}
        />
      </Section>
    ),
    saving: () => (
      <Section title={t('settings.cat.saving')}>
        <Switch
          on={settings.saveOnEdit}
          label={t('settings.saveOnEdit')}
          hint={t('settings.saveOnEditHint')}
          onToggle={() => void update({ saveOnEdit: !settings.saveOnEdit })}
        />
        <Switch
          on={settings.backupOnSave}
          label={t('settings.backupOnSave')}
          hint={t('settings.backupOnSaveHint')}
          onToggle={() => void update({ backupOnSave: !settings.backupOnSave })}
        />
        <label className="select-row with-hint">
          <span className="switch-text">
            <span>{t('settings.lockUnsaved')}</span>
            <span className="switch-hint">{t('settings.lockUnsavedHint')}</span>
          </span>
          <select
            className="select"
            value={settings.lockUnsaved}
            onChange={(e) => void update({ lockUnsaved: e.target.value === 'ask' ? 'ask' : 'save' })}
          >
            <option value="save">{t('settings.lockUnsaved.save')}</option>
            <option value="ask">{t('settings.lockUnsaved.ask')}</option>
          </select>
        </label>
      </Section>
    ),
    shortcuts: () => (
      <>
        <Section title={t('settings.globalHotkeys')} hint={t('settings.globalHotkeysHint')}>
          <HotkeyField
            label={t('settings.quickSearchHotkey')}
            value={settings.quickSearchHotkey}
            defaultValue={DEFAULT_SETTINGS.quickSearchHotkey}
            state={hotkeys?.quickSearch}
            onSave={(text) => update({ quickSearchHotkey: text })}
          />
          <HotkeyField
            label={t('settings.autotypeHotkey')}
            value={settings.autotypeHotkey}
            defaultValue={DEFAULT_SETTINGS.autotypeHotkey}
            state={hotkeys?.autotype}
            onSave={(text) => update({ autotypeHotkey: text })}
          />
          <HotkeyField
            label={t('settings.quickSaveHotkey')}
            value={settings.quickSaveHotkey}
            defaultValue={DEFAULT_SETTINGS.quickSaveHotkey}
            state={hotkeys?.quickSave}
            onSave={(text) => update({ quickSaveHotkey: text })}
          />
          <HotkeyField
            label={t('settings.lockHotkey')}
            value={settings.lockHotkey}
            defaultValue={DEFAULT_SETTINGS.lockHotkey}
            state={hotkeys?.lock}
            onSave={(text) => update({ lockHotkey: text })}
          />
        </Section>
        <Section title={t('settings.windowShortcuts')} hint={t('settings.windowShortcutsHint')}>
          {SHORTCUT_ACTIONS.map((a) => {
            const conflict = conflicts[a];
            return (
              <HotkeyField
                key={a}
                label={t(`settings.shortcut.${a}` as MessageKey)}
                value={settings.shortcuts[a]}
                defaultValue={DEFAULT_SETTINGS.shortcuts[a]}
                note={
                  conflict === 'duplicate'
                    ? t('settings.shortcutDuplicate')
                    : conflict === 'global'
                      ? t('settings.shortcutGlobal')
                      : null
                }
                validate={(text) => (usableShortcut(text) ? null : t('settings.shortcutInvalid'))}
                onSave={(text) => update({ shortcuts: { ...settings.shortcuts, [a]: text } })}
              />
            );
          })}
          {shortcutsChanged && (
            <div className="settings-actions">
              <button
                type="button"
                className="btn"
                onClick={() => void update({ shortcuts: DEFAULT_SETTINGS.shortcuts })}
              >
                {t('settings.restoreAllShortcuts')}
              </button>
            </div>
          )}
        </Section>
      </>
    ),
    autotype: () => (
      <Section title={t('settings.cat.autotype')}>
        {select(
          t('settings.keyDelay'),
          settings.autotypeKeyDelayMs,
          KEY_DELAYS,
          (ms) => t('settings.milliseconds', { n: ms }),
          (n) => void update({ autotypeKeyDelayMs: n }),
        )}
      </Section>
    ),
    system: () => (
      <Section title={t('settings.cat.system')}>
        <Switch
          on={autostart === true}
          label={t('settings.autostart')}
          hint={t('settings.autostartHint')}
          onToggle={toggleAutostart}
        />
      </Section>
    ),
    about: () => (
      <Section title="Mikoshi" hint={info ? t('settings.version', { v: info.version }) : undefined}>
        {(
          [
            ['notices', t('settings.about.notices'), t('settings.about.noticesHint')],
            ['settingsFolder', t('settings.about.settingsFolder'), undefined],
            ['logFolder', t('settings.about.logFolder'), undefined],
          ] as const
        ).map(([which, label, hint]) => (
          <div className="select-row" key={which}>
            <span className="switch-text">
              <span>{label}</span>
              {hint && <span className="switch-hint">{hint}</span>}
            </span>
            <button type="button" className="btn" onClick={() => void api.openAppFile(which).catch(fail)}>
              {t('settings.about.open')}
            </button>
          </div>
        ))}
        <p className="settings-note">{t('settings.about.offline')}</p>
      </Section>
    ),
  };

  return (
    <div
      className="settings-page"
      role="dialog"
      aria-modal="true"
      aria-label={t('settings.title')}
      tabIndex={-1}
      ref={root}
      onKeyDown={(e) => {
        // the window's shortcuts must not act on what is behind the page
        e.stopPropagation();
        const own = (e.target as HTMLElement).closest('[data-own-escape]');
        if (e.key === 'Escape' && !own) {
          e.preventDefault();
          onClose();
        } else if ((e.ctrlKey || e.metaKey) && e.key === ',') {
          e.preventDefault();
          onClose();
        }
      }}
    >
      <nav className="settings-nav" aria-label={t('settings.title')}>
        <button type="button" className="settings-back" onClick={onClose}>
          <Icon name="chevron" size={14} className="back" />
          {t('settings.back')}
        </button>
        <h1 className="settings-heading">{t('settings.title')}</h1>
        {CATEGORIES.map((c) => (
          <button
            key={c}
            type="button"
            className={c === cat ? 'settings-cat on' : 'settings-cat'}
            aria-current={c === cat ? 'page' : undefined}
            onClick={() => setCat(c)}
          >
            <Icon name={CATEGORY_ICONS[c]} />
            <span>{t(`settings.cat.${c}`)}</span>
          </button>
        ))}
      </nav>
      <div className="settings-content" key={cat}>
        {content[cat]()}
      </div>
    </div>
  );
}

/** The gear in the title bar; opens the settings page (also Ctrl+,). */
export function SettingsButton() {
  const { t } = usePrefs();
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => {
    setOpen(false);
    button.current?.focus();
  }, []);

  return (
    <>
      <button
        ref={button}
        className={open ? 'tb-btn icon-only active' : 'tb-btn icon-only'}
        title={`${t('toolbar.settings')} (Ctrl+,)`}
        aria-label={t('toolbar.settings')}
        aria-expanded={open}
        onClick={() => (open ? close() : setOpen(true))}
        data-settings-toggle
      >
        <Icon name="settings" />
      </button>
      {open && createPortal(<SettingsPage onClose={close} />, document.body)}
    </>
  );
}
