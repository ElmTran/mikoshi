import { useCallback, useRef, useState, type MouseEvent } from 'react';
import { usePrefs } from '../app/contexts';
import type { LanguagePref, ThemePref } from '../api/types';
import { Icon, type IconName } from './Icon';
import { useDismiss } from './useDismiss';

const THEMES: { value: ThemePref; icon: IconName }[] = [
  { value: 'system', icon: 'monitor' },
  { value: 'light', icon: 'sun' },
  { value: 'dark', icon: 'moon' },
];
const LANGUAGES: LanguagePref[] = ['system', 'zh-CN', 'en-US'];

/** Small flyout under the title bar: appearance, language, unlock options. */
export function SettingsButton() {
  const { t, settings, update, info } = usePrefs();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, root, close);

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
            <button
              type="button"
              role="switch"
              aria-checked={settings.rememberKeyFiles}
              className="switch-row"
              onClick={() => void update({ rememberKeyFiles: !settings.rememberKeyFiles })}
            >
              <span className="switch-text">
                <span>{t('settings.rememberKeyFiles')}</span>
                <span className="switch-hint">{t('settings.rememberKeyFilesHint')}</span>
              </span>
              <span className={settings.rememberKeyFiles ? 'switch on' : 'switch'} aria-hidden="true" />
            </button>
          </div>
          {info && <div className="flyout-foot">{t('settings.version', { v: info.version })}</div>}
        </div>
      )}
    </div>
  );
}
