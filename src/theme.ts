import type { ThemePref } from './api/types';
import { appWindow } from './api/window';

export type EffectiveTheme = 'light' | 'dark';

const darkQuery = () => window.matchMedia('(prefers-color-scheme: dark)');
const reducedMotion = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

export function systemTheme(): EffectiveTheme {
  return darkQuery().matches ? 'dark' : 'light';
}

/**
 * Applies a theme preference: tells the native window (so its frame and
 * shadow follow), then sets `data-theme` on <html>, which drives every
 * colour token. For "system" the answer comes from the window itself: right
 * after the override is removed, the page's own media query can still be stale.
 */
export async function applyTheme(pref: ThemePref): Promise<EffectiveTheme> {
  await appWindow.setTheme(pref === 'system' ? null : pref).catch(() => {});
  const theme = pref === 'system' ? ((await appWindow.theme().catch(() => null)) ?? systemTheme()) : pref;
  document.documentElement.dataset.theme = theme;
  return theme;
}

export function onSystemThemeChange(fn: () => void): () => void {
  const q = darkQuery();
  q.addEventListener('change', fn);
  return () => q.removeEventListener('change', fn);
}

/**
 * Runs a theme change as a circle that grows from `origin` (the button the
 * user clicked). Falls back to an instant switch without View Transitions or
 * with reduced motion.
 */
export async function revealThemeChange(
  origin: { x: number; y: number } | null,
  change: () => Promise<unknown>,
) {
  if (!origin || !document.startViewTransition || reducedMotion()) {
    await change();
    return;
  }
  const { x, y } = origin;
  const r = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
  const root = document.documentElement;
  try {
    const transition = document.startViewTransition(async () => {
      await change();
    });
    await transition.ready;
    root.animate(
      { clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${r}px at ${x}px ${y}px)`] },
      { duration: 480, easing: 'cubic-bezier(.2, 0, 0, 1)', pseudoElement: '::view-transition-new(root)' },
    );
    await transition.finished;
  } catch {
    // a skipped transition still applies the change
  }
}
