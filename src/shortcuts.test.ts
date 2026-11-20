import { describe, expect, it } from 'vitest';
import { DEFAULT_SHORTCUTS, shortcutConflicts, shortcutFor, usableShortcut } from './shortcuts';

const key = (
  code: string,
  mods: { ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean } = {},
) => ({
  code,
  ctrlKey: Boolean(mods.ctrl),
  altKey: Boolean(mods.alt),
  shiftKey: Boolean(mods.shift),
  metaKey: Boolean(mods.meta),
});

describe('main-window shortcuts', () => {
  it('finds the action for a key press', () => {
    expect(shortcutFor(key('KeyB', { ctrl: true }), DEFAULT_SHORTCUTS)).toBe('copyUsername');
    expect(shortcutFor(key('KeyU', { ctrl: true, shift: true }), DEFAULT_SHORTCUTS)).toBe('openUrl');
    expect(shortcutFor(key('Delete'), DEFAULT_SHORTCUTS)).toBe('deleteEntry');
    expect(shortcutFor(key('KeyB'), DEFAULT_SHORTCUTS)).toBeNull();
    expect(shortcutFor(key('ControlLeft', { ctrl: true }), DEFAULT_SHORTCUTS)).toBeNull();
  });

  it('follows changed and switched-off shortcuts', () => {
    const changed = { ...DEFAULT_SHORTCUTS, copyUsername: 'Alt+1', lock: '' };
    expect(shortcutFor(key('Digit1', { alt: true }), changed)).toBe('copyUsername');
    expect(shortcutFor(key('KeyB', { ctrl: true }), changed)).toBeNull();
    expect(shortcutFor(key('KeyL', { ctrl: true }), changed)).toBeNull();
  });

  it('accepts only keys that never type a character', () => {
    for (const ok of ['Ctrl+B', 'Alt+F4', 'Win+J', 'Delete', 'F2', 'Shift+F10', 'Insert']) {
      expect(usableShortcut(ok), ok).toBe(true);
    }
    for (const bad of ['', 'B', 'Shift+B', '1', 'Space', 'Enter', 'Tab', 'Escape', 'Ctrl+,']) {
      expect(usableShortcut(bad), bad).toBe(false);
    }
  });

  it('reports two actions on the same keys and keys taken by global hotkeys', () => {
    expect(shortcutConflicts(DEFAULT_SHORTCUTS, ['Ctrl+Alt+K', 'Ctrl+Alt+L', 'Ctrl+Alt+V'])).toEqual({});
    const clash = { ...DEFAULT_SHORTCUTS, copyTotp: 'Ctrl+B', lock: 'Ctrl+Alt+L' };
    expect(shortcutConflicts(clash, ['Ctrl+Alt+K', 'Ctrl+Alt+L'])).toEqual({
      copyUsername: 'duplicate',
      copyTotp: 'duplicate',
      lock: 'global',
    });
  });
});
