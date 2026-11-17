import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { api, listen } from '../api';
import type { AppInfo, Settings } from '../api/types';
import { formatters, resolveLocale, translate, translatePlural } from '../i18n/core';
import {
  applyTheme,
  onSystemThemeChange,
  revealThemeChange,
  systemTheme,
  type EffectiveTheme,
} from '../theme';
import { PrefsContext, type Prefs } from './contexts';

const DEFAULTS: Settings = { theme: 'system', language: 'system' };

/** Loads settings once, keeps them in sync across windows, applies theme and language. */
export function PrefsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [theme, setTheme] = useState<EffectiveTheme>(systemTheme);
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    api.getSettings().then(setSettings, () => setSettings(DEFAULTS));
    api.appInfo().then(setInfo, () => {});
    const off = listen<Settings>('settings-changed', setSettings);
    return () => void off.then((f) => f());
  }, []);

  const themePref = settings?.theme;
  useEffect(() => {
    if (!themePref) return;
    let live = true;
    applyTheme(themePref).then((t) => live && setTheme(t));
    const off = onSystemThemeChange(() => {
      if (themePref === 'system') applyTheme('system').then((t) => live && setTheme(t));
    });
    return () => {
      live = false;
      off();
    };
  }, [themePref]);

  const locale = resolveLocale(settings?.language ?? 'system');
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  useEffect(() => {
    document.documentElement.classList.toggle('effects', Boolean(info?.effects));
  }, [info]);

  const update = useCallback(
    async (patch: Partial<Settings>, origin?: { x: number; y: number }) => {
      if (!settings) return;
      const next = { ...settings, ...patch };
      if (patch.theme && patch.theme !== settings.theme) {
        // no reveal when the window already looks like the chosen theme
        const visible = next.theme === 'system' || next.theme !== theme;
        await revealThemeChange(visible ? (origin ?? null) : null, async () =>
          setTheme(await applyTheme(next.theme)),
        );
      }
      setSettings(next);
      await api.setSettings(next);
    },
    [settings, theme],
  );

  const value = useMemo<Prefs | null>(
    () =>
      settings && {
        settings,
        update,
        theme,
        locale,
        t: (key, vars) => translate(locale, key, vars),
        tn: (key, n, vars) => translatePlural(locale, key, n, vars),
        fmt: formatters(locale),
        info,
      },
    [settings, update, theme, locale, info],
  );

  if (!value) return null;
  return <PrefsContext.Provider value={value}>{children}</PrefsContext.Provider>;
}
