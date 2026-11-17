import { KEEPASS_ICONS } from '../icons/keepass';

/** A KeePass standard icon (0–68) in Mikoshi's line style. */
export function KpIcon({
  id,
  size = 16,
  className,
}: {
  id: number;
  size?: number;
  className?: string | undefined;
}) {
  const paths = KEEPASS_ICONS[id] ?? KEEPASS_ICONS[0] ?? [];
  return (
    <svg
      className={className ? `icon ${className}` : 'icon'}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      aria-hidden="true"
      focusable="false"
    >
      {paths.map((d, i) => (
        <path key={i} d={d} />
      ))}
    </svg>
  );
}
