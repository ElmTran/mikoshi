import { useState } from 'react';
import { useCustomIcons } from '../app/contexts';
import { KpIcon } from './KpIcon';

export function AppMark({ size = 18 }: { size?: number }) {
  return (
    <span className="app-mark" style={{ width: size, height: size }} aria-hidden="true">
      <svg viewBox="0 0 24 24" width={size * 0.62} height={size * 0.62}>
        <path
          fill="currentColor"
          d="M12 4.2a3.4 3.4 0 0 1 1.9 6.25l.95 6.85a.7.7 0 0 1-.7.8H9.85a.7.7 0 0 1-.7-.8l.95-6.85A3.4 3.4 0 0 1 12 4.2z"
        />
      </svg>
    </span>
  );
}

const GLYPH = { sm: 14, md: 18, lg: 28 } as const;

/** A stable colour from the title, so each entry keeps its tile colour. */
function hue(title: string) {
  let h = 0;
  for (const ch of title) h = (h * 31 + (ch.codePointAt(0) ?? 0)) >>> 0;
  return h % 6;
}

/**
 * An entry's icon: its custom icon if the database has one, otherwise its
 * KeePass standard icon on a coloured tile.
 */
export function EntryIcon({
  icon,
  customIcon,
  title,
  size = 'md',
}: {
  icon: number;
  customIcon: string | null;
  title: string;
  size?: 'sm' | 'md' | 'lg';
}) {
  const icons = useCustomIcons();
  const [broken, setBroken] = useState<string | null>(null);
  const src = customIcon && customIcon !== broken ? icons.get(customIcon) : undefined;
  if (src) {
    return (
      <span className={`avatar ${size} custom`} aria-hidden="true">
        <img src={src} alt="" draggable={false} onError={() => setBroken(customIcon)} />
      </span>
    );
  }
  return (
    <span className={`avatar ${size} hue-${hue(title)}`} aria-hidden="true">
      <KpIcon id={icon} size={GLYPH[size]} />
    </span>
  );
}

/** A group's icon, drawn bare (no tile) in the group tree. */
export function GroupIcon({ icon, customIcon }: { icon: number; customIcon: string | null }) {
  const icons = useCustomIcons();
  const [broken, setBroken] = useState<string | null>(null);
  const src = customIcon && customIcon !== broken ? icons.get(customIcon) : undefined;
  return src ? (
    <img className="group-img" src={src} alt="" draggable={false} onError={() => setBroken(customIcon)} />
  ) : (
    <KpIcon id={icon} />
  );
}

/** Shortcut text such as "Ctrl+B", shown inline. */
export function Keys({ keys }: { keys: string[] }) {
  return (
    <span className="keys">
      {keys.map((k, i) => (
        <kbd key={i}>{k}</kbd>
      ))}
    </span>
  );
}
