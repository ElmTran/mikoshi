import { describe, expect, it } from 'vitest';
import { isBrowserShortcut } from './appShell';

const key = (k: string, mods: { ctrl?: boolean; shift?: boolean } = {}) => ({
  key: k,
  ctrlKey: Boolean(mods.ctrl),
  metaKey: false,
  shiftKey: Boolean(mods.shift),
});

describe('isBrowserShortcut', () => {
  it('catches browser features', () => {
    expect(isBrowserShortcut(key('F5'))).toBe(true);
    expect(isBrowserShortcut(key('F12'))).toBe(true);
    expect(isBrowserShortcut(key('p', { ctrl: true }))).toBe(true);
    expect(isBrowserShortcut(key('R', { ctrl: true }))).toBe(true);
    expect(isBrowserShortcut(key('I', { ctrl: true, shift: true }))).toBe(true);
  });

  it('leaves typing and text editing alone', () => {
    expect(isBrowserShortcut(key('p'))).toBe(false);
    expect(isBrowserShortcut(key('c', { ctrl: true }))).toBe(false);
    expect(isBrowserShortcut(key('v', { ctrl: true }))).toBe(false);
    expect(isBrowserShortcut(key('a', { ctrl: true }))).toBe(false);
    expect(isBrowserShortcut(key('z', { ctrl: true }))).toBe(false);
    expect(isBrowserShortcut(key('Enter'))).toBe(false);
  });
});
