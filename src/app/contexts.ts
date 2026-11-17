import { createContext, useContext } from 'react';
import type { AppInfo, Settings } from '../api/types';
import type { Formatters, Locale, MessageKey, PluralKey, Vars } from '../i18n/core';
import type { EffectiveTheme } from '../theme';

export interface Prefs {
  settings: Settings;
  /** `origin`: where the user clicked, for the theme reveal animation */
  update: (patch: Partial<Settings>, origin?: { x: number; y: number }) => Promise<void>;
  theme: EffectiveTheme;
  locale: Locale;
  t: (key: MessageKey, vars?: Vars) => string;
  tn: (key: PluralKey, n: number, vars?: Vars) => string;
  fmt: Formatters;
  info: AppInfo | null;
}

export const PrefsContext = createContext<Prefs | null>(null);

export function usePrefs(): Prefs {
  const p = useContext(PrefsContext);
  if (!p) throw new Error('usePrefs outside PrefsProvider');
  return p;
}

export interface ToastOptions {
  message: string;
  /** seconds until the clipboard clears; shows a draining bar */
  clearsAfter?: number | null;
  tone?: 'success' | 'info' | 'error';
}

export const ToastContext = createContext<(t: ToastOptions) => void>(() => {});

export const useToast = () => useContext(ToastContext);

/** Custom icons of the unlocked database: id → data URL. Empty while locked. */
export const CustomIconsContext = createContext<ReadonlyMap<string, string>>(new Map());

export const useCustomIcons = () => useContext(CustomIconsContext);
