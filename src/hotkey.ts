// Global hotkeys as the settings store them; the Rust side (hotkeys.rs)
// parses the same format.

const MODIFIER_CODES = /^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/;

/**
 * The settings format (see hotkeys.rs): modifiers in the order Ctrl, Alt,
 * Shift, Win, then the key as `KeyboardEvent.code` with "Key"/"Digit" dropped.
 * `null` while only modifiers are down.
 */
export function hotkeyFromEvent(
  e: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>,
) {
  if (!e.code || MODIFIER_CODES.test(e.code)) return null;
  const key = e.code.replace(/^Key(?=[A-Z]$)/, '').replace(/^Digit(?=\d$)/, '');
  const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Win'];
  return [...mods.filter(Boolean), key].join('+');
}

/** Needs Ctrl, Alt or Win: anything less would swallow a key in every program. */
export const usableHotkey = (text: string) => /(^|\+)(Ctrl|Alt|Win)\+/.test(text);
