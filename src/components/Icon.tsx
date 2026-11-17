import type { ReactNode } from 'react';

// 24 × 24 line icons drawn for Mikoshi (stroke = currentColor).
const PATHS = {
  folder: (
    <path d="M3.5 7.5a2 2 0 0 1 2-2h3.7l2 2.2h7.3a2 2 0 0 1 2 2v7.8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z" />
  ),
  vault: (
    <>
      <rect x="3.5" y="4.5" width="17" height="15" rx="2.5" />
      <circle cx="12" cy="12" r="3.2" />
      <path d="M12 8.8V7M12 17v-1.8M15.2 12H17M7 12h1.8" />
    </>
  ),
  trash: <path d="M4.5 7h15M9.5 7V4.8h5V7M6.5 7l.9 12.2h9.2L17.5 7M10.2 10.5v5.5M13.8 10.5v5.5" />,
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6.2" />
      <path d="M15.2 15.2 20 20" />
    </>
  ),
  lock: (
    <>
      <rect x="5" y="10.5" width="14" height="9.5" rx="2.2" />
      <path d="M8.2 10.5V8a3.8 3.8 0 0 1 7.6 0v2.5M12 14.3v2" />
    </>
  ),
  min: <path d="M6.5 12h11" />,
  max: <rect x="6.5" y="6.5" width="11" height="11" rx="1.5" />,
  restore: (
    <path d="M8.5 6.5h8a1 1 0 0 1 1 1v8M6.5 9.5h8a1 1 0 0 1 1 1v7h-9a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1z" />
  ),
  close: <path d="M7 7l10 10M17 7 7 17" />,
  plus: <path d="M12 5.5v13M5.5 12h13" />,
  copy: (
    <>
      <rect x="8.5" y="8.5" width="11" height="11" rx="2" />
      <path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2" />
    </>
  ),
  check: <path d="M5.5 12.5l4 4 9-9" />,
  eye: (
    <>
      <path d="M2.8 12s3.4-6.2 9.2-6.2 9.2 6.2 9.2 6.2-3.4 6.2-9.2 6.2S2.8 12 2.8 12z" />
      <circle cx="12" cy="12" r="2.7" />
    </>
  ),
  eyeOff: (
    <path d="M4 4l16 16M9.9 6.1a9 9 0 0 1 2.1-.3c5.8 0 9.2 6.2 9.2 6.2a15 15 0 0 1-2.6 3.3M6.4 7.7A15 15 0 0 0 2.8 12s3.4 6.2 9.2 6.2a8.7 8.7 0 0 0 4-.9M10.1 10.2a2.7 2.7 0 0 0 3.7 3.7" />
  ),
  open: (
    <path d="M13.5 4.5h6v6M19.5 4.5 11 13M17.5 13.5V18a1.5 1.5 0 0 1-1.5 1.5H6A1.5 1.5 0 0 1 4.5 18V8A1.5 1.5 0 0 1 6 6.5h4.5" />
  ),
  keyboard: (
    <>
      <rect x="2.5" y="6" width="19" height="12" rx="2" />
      <path d="M6.5 10h.01M9.5 10h.01M12.5 10h.01M15.5 10h.01M18 10h.01M7.5 14h9" />
    </>
  ),
  edit: <path d="M4.5 19.5h3.8L19 8.8 15.2 5 4.5 15.7zM13.2 7l3.8 3.8" />,
  warn: <path d="M12 4.2 21 19.5H3zM12 10v4.2M12 16.9h.01" />,
  tag: (
    <>
      <path d="M4 11.6V5a1 1 0 0 1 1-1h6.6l8.3 8.3a1.2 1.2 0 0 1 0 1.7l-6.2 6.2a1.2 1.2 0 0 1-1.7 0z" />
      <circle cx="8.2" cy="8.2" r="1.3" />
    </>
  ),
  clip: (
    <path d="M16 7.5 9.2 14.3a2 2 0 0 0 2.8 2.8l7.1-7.1a4 4 0 0 0-5.6-5.7L6.3 11.5a6 6 0 0 0 8.5 8.5l5.3-5.3" />
  ),
  download: <path d="M12 4.5v10M7.5 10.5 12 15l4.5-4.5M5 19.5h14" />,
  clock: (
    <>
      <circle cx="12" cy="12" r="8.2" />
      <path d="M12 7.8V12l2.8 1.8" />
    </>
  ),
  key: (
    <>
      <circle cx="8" cy="15.5" r="3.6" />
      <path d="M10.6 13 19.5 4.1M16.3 7.3l2.4 2.4M13.9 9.7l1.8 1.8" />
    </>
  ),
  chevron: <path d="M7 10l5 5 5-5" />,
  sortDown: <path d="M12 5.5v13M7.5 14l4.5 4.5 4.5-4.5" />,
  folderOpen: (
    <path d="M3.5 17.5v-11a1.5 1.5 0 0 1 1.5-1.5h3.7l2 2.2h7.3a1.5 1.5 0 0 1 1.5 1.5V10M3.5 18.5l2.7-7a1.5 1.5 0 0 1 1.4-1h13a1 1 0 0 1 .9 1.4l-2.4 6a1.5 1.5 0 0 1-1.4 1.1H3.5z" />
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="2.8" />
      <path d="M12 3.5v2.2M12 18.3v2.2M20.5 12h-2.2M5.7 12H3.5M18 6l-1.6 1.6M7.6 16.4 6 18M18 18l-1.6-1.6M7.6 7.6 6 6" />
    </>
  ),
  bolt: <path d="M13 3.5 5.5 13.5h6l-1 7 7.5-10h-6z" />,
  sun: (
    <>
      <circle cx="12" cy="12" r="3.8" />
      <path d="M12 3v2M12 19v2M21 12h-2M5 12H3M18.4 5.6 17 7M7 17l-1.4 1.4M18.4 18.4 17 17M7 7 5.6 5.6" />
    </>
  ),
  moon: <path d="M19.5 14.5A7.5 7.5 0 0 1 9.5 4.5a7.5 7.5 0 1 0 10 10z" />,
  monitor: (
    <>
      <rect x="3.5" y="4.5" width="17" height="11.5" rx="1.8" />
      <path d="M9 19.5h6M12 16v3.5" />
    </>
  ),
  globe: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17M12 3.5c2.4 2.4 3.6 5.2 3.6 8.5s-1.2 6.1-3.6 8.5c-2.4-2.4-3.6-5.2-3.6-8.5S9.6 5.9 12 3.5z" />
    </>
  ),
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof PATHS;

export function Icon({
  name,
  size = 16,
  className,
}: {
  name: IconName;
  size?: number;
  className?: string | undefined;
}) {
  return (
    <svg
      className={className ? `icon ${className}` : 'icon'}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
