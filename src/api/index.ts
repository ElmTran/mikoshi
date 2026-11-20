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
  type Deleted,
  type DialogText,
  type EditableEntry,
  type EntryDetail,
  type EntryEdit,
  type EntrySummary,
  type ErrorCode,
  type Generated,
  type GeneratorOptions,
  type GroupView,
  type HistoryItem,
  type HotkeyStatus,
  type SaveOptions,
  type Settings,
  type Strength,
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
  /** `discard`: unsaved edits may be dropped */
  selectDatabase: (path: string, discard = false) => call<VaultStatus>('select_database', { path, discard }),
  /** null when the dialog was cancelled */
  chooseDatabase: (text: DialogText) => call<VaultStatus | null>('choose_database', { text }),
  chooseKeyFile: (text: DialogText) => call<string | null>('choose_key_file', { text }),
  /** an empty password counts as none; `keyFile` is a path */
  unlock: (password: string, keyFile: string | null) =>
    call<VaultStatus>('unlock', { password: password || null, keyFile }),
  forgetRecent: (path: string) => call<Settings>('forget_recent', { path }),
  /** `discard`: unsaved edits may be dropped; otherwise the setting decides */
  lock: (discard = false) => call<VaultStatus>('lock', { discard }),
  /** quits from the tray menu after the user answered; unsaved edits save unless discarded */
  quitApp: (discard = false) => call<void>('quit_app', { discard }),
  listGroups: () => call<GroupView[]>('list_groups'),
  listEntries: (groupId: string) => call<EntrySummary[]>('list_entries', { groupId }),
  searchEntries: (query: string) => call<EntrySummary[]>('search_entries', { query }),
  getEntry: (id: string) => call<EntryDetail>('get_entry', { id }),
  getCustomIcons: () => call<CustomIcon[]>('get_custom_icons'),
  revealField: (entryId: string, key: string) => call<string>('reveal_field', { entryId, key }),
  /** the entry with every value, for the editor; drop it when editing ends */
  getEntryForEdit: (entryId: string) => call<EditableEntry>('get_entry_for_edit', { entryId }),
  /** creates an entry in a group; returns its id */
  createEntry: (groupId: string, edit: EntryEdit) => call<string>('create_entry', { groupId, edit }),
  /** applies an edit; false when it changed nothing */
  saveEntry: (entryId: string, edit: EntryEdit) => call<boolean>('save_entry', { entryId, edit }),
  listHistory: (entryId: string) => call<HistoryItem[]>('list_history', { entryId }),
  getHistoryEntry: (entryId: string, index: number) =>
    call<EntryDetail>('get_history_entry', { entryId, index }),
  revealHistoryField: (entryId: string, index: number, key: string) =>
    call<string>('reveal_history_field', { entryId, index, key }),
  restoreHistory: (entryId: string, index: number) =>
    call<VaultStatus>('restore_history', { entryId, index }),
  /** writes the database to its file; a conflict or upgrade shows up as an error code */
  saveDatabase: (options?: SaveOptions) => call<VaultStatus>('save_database', { options }),
  /** reads the file again, dropping unsaved edits */
  reloadDatabase: () => call<VaultStatus>('reload_database'),
  /** whether deleting these is for good (they are in the bin, or it is off) */
  deletesPermanently: (entryIds: string[], groupIds: string[]) =>
    call<boolean>('deletes_permanently', { entryIds, groupIds }),
  /** `binName`: the recycle bin's name if it has to be made */
  deleteEntries: (entryIds: string[], binName: string) =>
    call<Deleted>('delete_entries', { entryIds, binName }),
  deleteGroup: (groupId: string, binName: string) => call<Deleted>('delete_group', { groupId, binName }),
  /** takes an entry or group out of the recycle bin */
  restoreItem: (id: string) => call<void>('restore_item', { id }),
  emptyRecycleBin: () => call<number>('empty_recycle_bin'),
  createGroup: (parentId: string, name: string) => call<string>('create_group', { parentId, name }),
  renameGroup: (groupId: string, name: string) => call<void>('rename_group', { groupId, name }),
  moveGroup: (groupId: string, to: string) => call<void>('move_group', { groupId, to }),
  moveEntries: (entryIds: string[], to: string) => call<void>('move_entries', { entryIds, to }),
  /** false when the save dialog was cancelled */
  exportAttachment: (entryId: string, name: string, text: DialogText) =>
    call<boolean>('export_attachment', { entryId, name, text }),
  /** the name the attachment got; null when the dialog was cancelled */
  addAttachment: (entryId: string, text: DialogText) =>
    call<string | null>('add_attachment', { entryId, text }),
  removeAttachment: (entryId: string, name: string) => call<void>('remove_attachment', { entryId, name }),
  /** where a new database goes; null when cancelled */
  chooseNewDatabase: (text: DialogText, fileName: string) =>
    call<string | null>('choose_new_database', { text, fileName }),
  /** makes a new key file; its path, or null when cancelled */
  createKeyFile: (text: DialogText, fileName: string) =>
    call<string | null>('create_key_file', { text, fileName }),
  createDatabase: (path: string, name: string, password: string, keyFile: string | null) =>
    call<VaultStatus>('create_database', { path, name, password: password || null, keyFile }),
  generatePassword: (options: GeneratorOptions) => call<Generated>('generate_password', { options }),
  passwordStrength: (password: string) => call<Strength>('password_strength', { password }),
  /** copies a value already on screen (a generated password) as a secret */
  copySecret: (value: string) => call<CopyResult>('copy_secret', { value }),
  copyField: (entryId: string, key: string) => call<CopyResult>('copy_field', { entryId, key }),
  getTotp: (entryId: string) => call<TotpView>('get_totp', { entryId }),
  /** opens the entry's URL in the default browser (http and https only) */
  openUrl: (entryId: string) => call<void>('open_url', { entryId }),
  showQuickSearch: () => call<void>('show_quick_search'),
  /** `restore`: give the focus back to the window that was in front before */
  hideQuickSearch: (restore: boolean) => call<void>('hide_quick_search', { restore }),
  /** entries that fit the window the quick search box was opened over */
  autotypeCandidates: () => call<EntrySummary[]>('autotype_candidates'),
  /** from the quick search box: the entry's sequence, or only the password */
  autotypeEntry: (entryId: string, passwordOnly: boolean) =>
    call<void>('autotype_entry', { entryId, passwordOnly }),
  /** from the main window: it minimizes and types into the window behind */
  autotypeFromMain: (entryId: string) => call<void>('autotype_from_main', { entryId }),
  hotkeyStatus: () => call<HotkeyStatus>('hotkey_status'),
  /** while recording a new hotkey; saving one also ends the pause */
  pauseHotkeys: (paused: boolean) => call<HotkeyStatus>('pause_hotkeys', { paused }),
  getAutostart: () => call<boolean>('get_autostart'),
  setAutostart: (enabled: boolean) => call<boolean>('set_autostart', { enabled }),
};

export type AppEvent =
  | 'vault-changed'
  | 'vault-edited'
  | 'settings-changed'
  | 'quick-shown'
  | 'hotkeys-changed'
  | 'autotype-failed'
  | 'save-conflict-copy'
  | 'quit-requested';

/** Subscribes to an event sent by the Rust side. Returns an unsubscribe function. */
export async function listen<T>(event: AppEvent, handler: (payload: T) => void): Promise<() => void> {
  if (isTauri) return tauriListen<T>(event, (e) => handler(e.payload));
  if (import.meta.env.DEV) {
    const mock = await import('./mock');
    return mock.listen(event, handler as (payload: unknown) => void);
  }
  return () => {};
}
