import { useCallback, useRef, useState } from 'react';
import { usePrefs } from '../app/contexts';
import { KEEPASS_ICON_COUNT, KEEPASS_ICON_NAMES } from '../icons/keepass';
import { EntryIcon } from './bits';
import { KpIcon } from './KpIcon';
import { useDismiss } from './useDismiss';

interface Props {
  icon: number;
  customIcon: string | null;
  title: string;
  /** null while the icon is left as it is */
  picked: number | null;
  onPick: (icon: number) => void;
}

/** The entry's icon as a button; opens a grid of the 69 KeePass icons. */
export function IconPicker({ icon, customIcon, title, picked, onPick }: Props) {
  const { t } = usePrefs();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, root, close);
  // a picked standard icon replaces a custom one
  const shown = picked ?? icon;

  return (
    <div className="icon-picker" ref={root}>
      <button
        type="button"
        className="icon-picker-btn"
        title={t('editor.iconPick')}
        aria-label={t('editor.iconPick')}
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <EntryIcon icon={shown} customIcon={picked === null ? customIcon : null} title={title} size="lg" />
      </button>
      {open && (
        <div className="flyout icon-grid" role="listbox" aria-label={t('editor.iconPick')}>
          {Array.from({ length: KEEPASS_ICON_COUNT }, (_, i) => (
            <button
              key={i}
              type="button"
              role="option"
              aria-selected={shown === i && (picked !== null || !customIcon)}
              className={shown === i && (picked !== null || !customIcon) ? 'on' : undefined}
              title={KEEPASS_ICON_NAMES[i]}
              onClick={() => {
                onPick(i);
                setOpen(false);
              }}
            >
              <KpIcon id={i} size={18} />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
