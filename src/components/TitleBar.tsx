import { useEffect, useState, type ReactNode } from 'react';
import { usePrefs } from '../app/contexts';
import { withShortcut } from '../shortcuts';
import { appWindow } from '../api/window';
import { AppMark } from './bits';
import { Icon } from './Icon';
import { SettingsButton } from './Settings';

interface Props {
  fileName?: string;
  dirty?: boolean;
  /** the search box, placed in the middle of the title bar */
  center?: ReactNode;
  onQuickSearch?: () => void;
  /** shows a save button; called by Ctrl+S and the button */
  onSave?: (() => void) | undefined;
  onLock?: () => void;
}

/** Custom title bar: drag area, file name, actions and the caption buttons. */
export function TitleBar({ fileName, dirty, center, onQuickSearch, onSave, onLock }: Props) {
  const { t, info, settings } = usePrefs();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const sync = () => void appWindow.isMaximized().then(setMaximized);
    sync();
    const off = appWindow.onResized(sync);
    return () => void off.then((f) => f());
  }, []);

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="tb-brand" data-tauri-drag-region>
        <AppMark size={22} />
        <span data-tauri-drag-region>Mikoshi</span>
      </div>
      {fileName && (
        <div className="tb-file" data-tauri-drag-region>
          <span data-tauri-drag-region>{fileName}</span>
          {dirty && <span className="dirty" title={t('title.unsaved')} />}
        </div>
      )}
      {info?.demo && (
        <span className="demo-badge" title={t('app.demoTip')}>
          {t('app.demoBadge')}
        </span>
      )}
      <div className="tb-center" data-tauri-drag-region>
        {center}
      </div>
      <div className="tb-actions">
        {onQuickSearch && (
          <button
            className="tb-btn icon-only"
            title={withShortcut(t('toolbar.quickSearch'), settings.quickSearchHotkey)}
            aria-label={t('toolbar.quickSearch')}
            onClick={onQuickSearch}
          >
            <Icon name="bolt" />
          </button>
        )}
        {onSave && (
          <button
            className="tb-btn save"
            title={withShortcut(t('editor.save'), settings.shortcuts.save)}
            onClick={onSave}
          >
            <Icon name="check" />
            <span>{t('editor.save')}</span>
          </button>
        )}
        <SettingsButton />
        {onLock && (
          <button
            className="tb-btn"
            title={withShortcut(t('toolbar.lock'), settings.shortcuts.lock)}
            onClick={onLock}
          >
            <Icon name="lock" />
            <span>{t('toolbar.lock')}</span>
          </button>
        )}
      </div>
      <div className="caption">
        <button
          title={t('caption.minimize')}
          aria-label={t('caption.minimize')}
          onClick={() => void appWindow.minimize()}
        >
          <Icon name="min" />
        </button>
        <button
          title={t(maximized ? 'caption.restore' : 'caption.maximize')}
          aria-label={t(maximized ? 'caption.restore' : 'caption.maximize')}
          onClick={() => void appWindow.toggleMaximize()}
        >
          <Icon name={maximized ? 'restore' : 'max'} />
        </button>
        <button
          className="close"
          title={t('caption.close')}
          aria-label={t('caption.close')}
          onClick={() => void appWindow.close()}
        >
          <Icon name="close" />
        </button>
      </div>
    </header>
  );
}
