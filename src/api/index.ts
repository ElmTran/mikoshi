// The only way the UI talks to the Rust side. Inside Tauri every call goes
// through `invoke`; in a plain browser (`pnpm dev`, for UI work only) a mock
// backend with the same demo data stands in. The mock is excluded from
// production builds.

import { invoke } from '@tauri-apps/api/core';
import { listen as tauriListen } from '@tauri-apps/api/event';
import {
  AppError,
  ERROR_CODES,
  type AppInfo,
  type CopyResult,
  type CustomIcon,
  type DialogText,
  type EntryDetail,
  type EntrySummary,
  type ErrorCode,
  type GroupView,
  type Settings,
  type TotpView,
  type VaultStatus,
} from './types';

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function toAppError(cmd: string, e: unknown): AppError {
  if (e instanceof AppError) return e;
  if (
    e &&
    typeof e === 'object' &&
    'code' in e &&
    (ERROR_CODES as readonly string[]).includes((e as { code: ErrorCode }).code)
  ) {
    return new AppError((e as { code: ErrorCode }).code);
  }
  // Not one of our error codes: the call never reached the command (missing
  // permission, app not ready). Worth a trace. The arguments are never
  // logged: they can hold a password.
  console.error(`command "${cmd}" failed before it ran:`, e);
  return new AppError('internal');
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    if (isTauri) return await invoke<T>(cmd, args);
    if (import.meta.env.DEV) {
      const mock = await import('./mock');
      return await mock.call<T>(cmd, args);
    }
    throw new AppError('internal');
  } catch (e) {
    throw toAppError(cmd, e);
  }
}

export const api = {
  appInfo: () => call<AppInfo>('app_info'),
  getSettings: () => call<Settings>('get_settings'),
  setSettings: (settings: Settings) => call<Settings>('set_settings', { settings }),
  vaultStatus: () => call<VaultStatus>('vault_status'),
  selectDatabase: (path: string) => call<VaultStatus>('select_database', { path }),
  /** null when the dialog was cancelled */
  chooseDatabase: (text: DialogText) => call<VaultStatus | null>('choose_database', { text }),
  chooseKeyFile: (text: DialogText) => call<string | null>('choose_key_file', { text }),
  /** an empty password counts as none; `keyFile` is a path */
  unlock: (password: string, keyFile: string | null) =>
    call<VaultStatus>('unlock', { password: password || null, keyFile }),
  forgetRecent: (path: string) => call<Settings>('forget_recent', { path }),
  lock: () => call<VaultStatus>('lock'),
  listGroups: () => call<GroupView[]>('list_groups'),
  listEntries: (groupId: string) => call<EntrySummary[]>('list_entries', { groupId }),
  searchEntries: (query: string) => call<EntrySummary[]>('search_entries', { query }),
  getEntry: (id: string) => call<EntryDetail>('get_entry', { id }),
  getCustomIcons: () => call<CustomIcon[]>('get_custom_icons'),
  revealField: (entryId: string, key: string) => call<string>('reveal_field', { entryId, key }),
  copyField: (entryId: string, key: string) => call<CopyResult>('copy_field', { entryId, key }),
  getTotp: (entryId: string) => call<TotpView>('get_totp', { entryId }),
  /** opens the entry's URL in the default browser (http and https only) */
  openUrl: (entryId: string) => call<void>('open_url', { entryId }),
  showQuickSearch: () => call<void>('show_quick_search'),
  hideQuickSearch: () => call<void>('hide_quick_search'),
};

export type AppEvent = 'vault-changed' | 'settings-changed' | 'quick-shown';

/** Subscribes to an event sent by the Rust side. Returns an unsubscribe function. */
export async function listen<T>(event: AppEvent, handler: (payload: T) => void): Promise<() => void> {
  if (isTauri) return tauriListen<T>(event, (e) => handler(e.payload));
  if (import.meta.env.DEV) {
    const mock = await import('./mock');
    return mock.listen(event, handler as (payload: unknown) => void);
  }
  return () => {};
}
