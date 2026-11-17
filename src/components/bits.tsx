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

/** Letter tile for an entry. The colour is derived from the title, so it is stable. */
export function Avatar({ title, size = 'md' }: { title: string; size?: 'sm' | 'md' | 'lg' }) {
  const first = [...title.trim()][0] ?? '?';
  let h = 0;
  for (const ch of title) h = (h * 31 + (ch.codePointAt(0) ?? 0)) >>> 0;
  return (
    <span className={`avatar ${size} hue-${h % 6}`} aria-hidden="true">
      {/[a-z]/i.test(first) ? first.toUpperCase() : first}
    </span>
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
