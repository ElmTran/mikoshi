// Makes the page behave like an app window instead of a web page. Installed
// in release builds only, so reload and the inspector stay handy in dev.

const BROWSER_KEYS = new Set(['f3', 'f5', 'f7', 'f12']);
const BROWSER_CTRL_KEYS = new Set(['f', 'g', 'j', 'o', 'p', 'r', 's', 'u']);
const BROWSER_CTRL_SHIFT_KEYS = new Set(['c', 'i', 'j', 'r']);

/** True for browser shortcuts (find, print, reload, view source, inspector...) that must do nothing here. */
export function isBrowserShortcut(
  e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'shiftKey'>,
): boolean {
  const k = e.key.toLowerCase();
  const ctrl = e.ctrlKey || e.metaKey;
  if (BROWSER_KEYS.has(k)) return true;
  if (ctrl && e.shiftKey) return BROWSER_CTRL_SHIFT_KEYS.has(k);
  return ctrl && BROWSER_CTRL_KEYS.has(k);
}

export function installAppBehaviour() {
  // The browser's own menu (Back, Refresh, Save as, Print) has no place here.
  // Text boxes keep theirs for Cut / Copy / Paste.
  window.addEventListener('contextmenu', (e) => {
    const el = e.target instanceof Element ? e.target : null;
    if (!el?.closest('input, textarea')) e.preventDefault();
  });

  // Only the browser's default action is cancelled; the event still reaches
  // the app's own shortcut handlers (Ctrl+F, Ctrl+U, Ctrl+S, Ctrl+G...).
  window.addEventListener(
    'keydown',
    (e) => {
      if (isBrowserShortcut(e)) e.preventDefault();
    },
    true,
  );
}
