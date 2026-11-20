// Main-window shortcuts (features.md §5): which keys run which action. The
// settings keep them as text in the hotkey format (see hotkey.ts); "" = off.
// They are handled in the page, not registered with Windows.

import { DEFAULT_SETTINGS, type Shortcuts } from './api/types';
import { hotkeyFromEvent } from './hotkey';

export type ShortcutAction = keyof Shortcuts;

/** The order the settings page lists them in, with the i18n key of each label. */
export const SHORTCUT_ACTIONS: readonly ShortcutAction[] = [
  'search',
  'copyUsername',
  'copyPassword',
  'copyUrl',
  'openUrl',
  'copyTotp',
  'autotype',
  'togglePassword',
  'newEntry',
  'editEntry',
  'deleteEntry',
  'save',
  'lock',
  'generator',
];

/** The defaults, the same as settings.rs. */
export const DEFAULT_SHORTCUTS: Shortcuts = DEFAULT_SETTINGS.shortcuts;

/** Keys that may be a shortcut on their own: they never type a character. */
const LONE_KEYS = /^(Delete|Insert|F([1-9]|1[0-2]))$/;

/** Keys the window keeps for itself: moving around and typing. */
const RESERVED = new Set(['Tab', 'Shift+Tab', 'Escape', 'Enter', 'Ctrl+,', 'Ctrl+Alt+K', 'Backspace']);

/**
 * Whether `text` can be a main-window shortcut: Ctrl, Alt or Win with a key,
 * or one of the keys that type nothing (Delete, Insert, F1–F12), and none of
 * the keys the window needs for itself.
 */
export function usableShortcut(text: string): boolean {
  if (!text || RESERVED.has(text)) return false;
  if (/(^|\+)(Ctrl|Alt|Win)\+./.test(text)) return true;
  const key = text.replace(/^Shift\+/, '');
  return LONE_KEYS.test(key);
}

/** The action a key press runs, if any. */
export function shortcutFor(
  e: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>,
  shortcuts: Shortcuts,
): ShortcutAction | null {
  const text = hotkeyFromEvent(e);
  if (!text) return null;
  for (const action of SHORTCUT_ACTIONS) {
    if (shortcuts[action] === text) return action;
  }
  return null;
}

export type Conflict = 'duplicate' | 'global';

/**
 * Problems with a set of shortcuts: two actions on the same keys, or a key
 * that is also a global hotkey (Windows takes those first, so the window
 * would never see them).
 */
export function shortcutConflicts(
  shortcuts: Shortcuts,
  globals: readonly string[],
): Partial<Record<ShortcutAction, Conflict>> {
  const out: Partial<Record<ShortcutAction, Conflict>> = {};
  const seen = new Map<string, ShortcutAction>();
  for (const action of SHORTCUT_ACTIONS) {
    const text = shortcuts[action];
    if (!text) continue;
    if (globals.includes(text)) out[action] = 'global';
    const other = seen.get(text);
    if (other) {
      out[action] = 'duplicate';
      out[other] = 'duplicate';
    } else {
      seen.set(text, action);
    }
  }
  return out;
}

/** "Ctrl+Shift+U" → ["Ctrl", "Shift", "U"], for showing as key caps; `undefined` when off. */
export const shortcutKeys = (text: string): string[] | undefined => (text ? text.split('+') : undefined);

/** A tooltip with the shortcut after it: "Edit (Ctrl+E)"; just the label when off. */
export const withShortcut = (label: string, text: string) => (text ? `${label} (${text})` : label);
