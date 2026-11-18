import { describe, expect, it } from 'vitest';
import { hotkeyFromEvent, usableHotkey } from './hotkey';

const ev = (
  code: string,
  mods: Partial<Record<'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey', boolean>> = {},
) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe('hotkeyFromEvent', () => {
  it('writes modifiers in a fixed order and shortens letter and digit keys', () => {
    expect(hotkeyFromEvent(ev('KeyK', { altKey: true, ctrlKey: true }))).toBe('Ctrl+Alt+K');
    expect(hotkeyFromEvent(ev('Digit1', { metaKey: true, shiftKey: true }))).toBe('Shift+Win+1');
    expect(hotkeyFromEvent(ev('F5', { ctrlKey: true }))).toBe('Ctrl+F5');
    expect(hotkeyFromEvent(ev('Space', { altKey: true }))).toBe('Alt+Space');
    expect(hotkeyFromEvent(ev('Numpad1', { ctrlKey: true }))).toBe('Ctrl+Numpad1');
  });

  it('waits while only modifiers are down', () => {
    for (const code of ['ControlLeft', 'AltRight', 'ShiftLeft', 'MetaLeft', 'OSRight', ''])
      expect(hotkeyFromEvent(ev(code, { ctrlKey: true }))).toBeNull();
  });
});

describe('usableHotkey', () => {
  it('needs Ctrl, Alt or Win', () => {
    expect(usableHotkey('Ctrl+Alt+K')).toBe(true);
    expect(usableHotkey('Win+F1')).toBe(true);
    expect(usableHotkey('Alt+Space')).toBe(true);
    expect(usableHotkey('K')).toBe(false);
    expect(usableHotkey('Shift+K')).toBe(false);
  });
});
