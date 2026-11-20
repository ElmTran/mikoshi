// Mirrors the Rust view types in crates/mikoshi-core/src/model.rs and the
// command results in src-tauri/src/commands.rs. None of these ever carries a
// password, a protected field value or a TOTP secret.

export type GroupKind = 'root' | 'normal' | 'recycleBin';

/** Groups arrive in tree order (depth first, recycle bin last). */
export interface GroupView {
  id: string;
  name: string;
  parentId: string | null;
  depth: number;
  kind: GroupKind;
  entryCount: number;
  /** KeePass standard icon, 0–68 */
  icon: number;
  /** custom icon id; drawn instead of `icon` when it is known */
  customIcon: string | null;
  /** expanded in the tree, as the file remembers it */
  expanded: boolean;
  hasChildren: boolean;
}

export interface EntrySummary {
  id: string;
  groupId: string;
  title: string;
  username: string;
  url: string;
  /** ISO 8601, UTC (`...Z`); null when the file has no time */
  modified: string | null;
  expired: boolean;
  hasTotp: boolean;
  tags: string[];
  icon: number;
  customIcon: string | null;
}

export interface FieldView {
  key: string;
  protected: boolean;
  value: string | null;
}

export interface AttachmentView {
  name: string;
  size: number;
}

export interface EntryDetail {
  id: string;
  groupId: string;
  groupPath: string[];
  title: string;
  username: string;
  url: string;
  notes: string;
  tags: string[];
  created: string | null;
  modified: string | null;
  expires: string | null;
  expired: boolean;
  hasPassword: boolean;
  hasTotp: boolean;
  fields: FieldView[];
  attachments: AttachmentView[];
  icon: number;
  customIcon: string | null;
}

export interface CustomIcon {
  id: string;
  mime: string;
  /** base64 image bytes */
  data: string;
}

export interface TotpView {
  code: string;
  period: number;
  remaining: number;
}

export interface VaultStatus {
  locked: boolean;
  /** the selected database file; null before one is chosen */
  path: string | null;
  file: string | null;
  /** the database's own name once unlocked, the file name before */
  name: string;
  /** key file remembered for this database (a path, never contents) */
  keyFile: string | null;
  /** edits not saved yet */
  dirty: boolean;
}

export interface AppInfo {
  version: string;
  demo: boolean;
  /** started with Windows: the main window stays hidden until asked for */
  startHidden: boolean;
}

export interface CopyResult {
  clearsAfter: number | null;
}

/** Texts for the native file dialogs, in the UI language. */
export interface DialogText {
  title: string;
  filter: string;
  allFiles: string;
}

export type ThemePref = 'system' | 'light' | 'dark';
export type LanguagePref = 'system' | 'zh-CN' | 'en-US';
export type SortKey = 'title' | 'username' | 'url' | 'modified';

export interface RecentFile {
  path: string;
  keyFile: string | null;
}

export interface EntrySort {
  key: SortKey;
  desc: boolean;
}

export interface Settings {
  theme: ThemePref;
  language: LanguagePref;
  /** most recent first; managed by the Rust side */
  recent: RecentFile[];
  rememberKeyFiles: boolean;
  entrySort: EntrySort;
  /** seconds until a copied secret is cleared; 0 = never */
  clipboardClearSeconds: number;
  /** minutes without input anywhere before locking; 0 = never */
  idleLockMinutes: number;
  lockOnMinimize: boolean;
  /** global hotkeys like "Ctrl+Alt+K"; "" = off */
  quickSearchHotkey: string;
  lockHotkey: string;
  autotypeHotkey: string;
  /** milliseconds between auto-typed keys */
  autotypeKeyDelayMs: number;
  /** keep the previous file as NAME.bak before each save */
  backupOnSave: boolean;
  /** write every change to the file at once; off: changes wait for Ctrl+S */
  saveOnEdit: boolean;
  /** main-window shortcuts, hotkey format; "" = off */
  shortcuts: Shortcuts;
  /** unsaved edits when the database locks: save, or ask on a manual lock */
  lockUnsaved: LockUnsaved;
  /** the password generator's last choices */
  generator: GeneratorOptions;
}

export type GeneratorMode = 'password' | 'passphrase';

export interface GeneratorOptions {
  mode: GeneratorMode;
  length: number;
  lower: boolean;
  upper: boolean;
  digits: boolean;
  symbols: boolean;
  excludeLookAlike: boolean;
  words: number;
  separator: string;
  capitalize: boolean;
  addDigit: boolean;
}

export const DEFAULT_GENERATOR: GeneratorOptions = {
  mode: 'password',
  length: 20,
  lower: true,
  upper: true,
  digits: true,
  symbols: true,
  excludeLookAlike: false,
  words: 5,
  separator: '-',
  capitalize: false,
  addDigit: false,
};

/** zxcvbn's estimate: 0 (guessed at once) to 4 (very hard). */
export interface Strength {
  score: number;
  guessesLog10: number;
}

export interface Generated {
  password: string;
  /** entropy of the generator with these options */
  bits: number;
  strength: Strength;
}

/** What a delete did. */
export type Deleted = 'toRecycleBin' | 'permanently';

/** Main-window shortcuts (features.md §5); see shortcuts.ts. */
export interface Shortcuts {
  search: string;
  copyUsername: string;
  copyPassword: string;
  copyUrl: string;
  openUrl: string;
  copyTotp: string;
  autotype: string;
  togglePassword: string;
  newEntry: string;
  editEntry: string;
  deleteEntry: string;
  save: string;
  lock: string;
  generator: string;
}

export type LockUnsaved = 'save' | 'ask';

/** What became of a hotkey from the settings. */
export type HotkeyState = 'off' | 'active' | 'taken' | 'invalid';

export interface HotkeyStatus {
  quickSearch: HotkeyState;
  lock: HotkeyState;
  autotype: HotkeyState;
}

/** Payload of `quick-shown`. */
export interface QuickShown {
  /** auto-type: list the entries that fit the window in front, to pick one */
  pick: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: 'system',
  language: 'system',
  recent: [],
  rememberKeyFiles: true,
  entrySort: { key: 'modified', desc: true },
  clipboardClearSeconds: 10,
  idleLockMinutes: 5,
  lockOnMinimize: false,
  quickSearchHotkey: 'Ctrl+Alt+K',
  lockHotkey: 'Ctrl+Alt+L',
  autotypeHotkey: 'Ctrl+Alt+V',
  autotypeKeyDelayMs: 20,
  backupOnSave: true,
  saveOnEdit: true,
  shortcuts: {
    search: 'Ctrl+F',
    copyUsername: 'Ctrl+B',
    copyPassword: 'Ctrl+C',
    copyUrl: 'Ctrl+U',
    openUrl: 'Ctrl+Shift+U',
    copyTotp: 'Ctrl+T',
    autotype: 'Ctrl+Shift+V',
    togglePassword: 'Ctrl+H',
    newEntry: 'Ctrl+N',
    editEntry: 'Ctrl+E',
    deleteEntry: 'Delete',
    save: 'Ctrl+S',
    lock: 'Ctrl+L',
    generator: 'Ctrl+G',
  },
  lockUnsaved: 'save',
  generator: DEFAULT_GENERATOR,
};

// ---------------------------------------------------------------- editing

/** An entry with every value (secrets included), for the editor. */
export interface EditableEntry {
  id: string;
  groupId: string;
  title: string;
  username: string;
  password: string;
  url: string;
  notes: string;
  tags: string[];
  /** ISO 8601, UTC; null when it never expires */
  expires: string | null;
  fields: EditableField[];
  /** TOTP settings without the secret; null when there is none */
  totp: TotpSummary | null;
  icon: number;
  customIcon: string | null;
}

export interface EditableField {
  key: string;
  value: string;
  protected: boolean;
}

export interface TotpSummary {
  /** false when the settings cannot be used */
  valid: boolean;
  algorithm: string | null;
  digits: number | null;
  period: number | null;
  steam: boolean;
}

export interface HistoryItem {
  /** position in the file's history, oldest first */
  index: number;
  modified: string | null;
  title: string;
  username: string;
}

export interface FieldEdit {
  /** the name before the edit; null for a new field */
  original: string | null;
  key: string;
  value: string;
  protected: boolean;
}

export type TotpEdit =
  | { kind: 'unchanged' }
  | { kind: 'remove' }
  | { kind: 'uri'; uri: string }
  | { kind: 'manual'; secret: string; algorithm: string; digits: number; period: number };

export interface EntryEdit {
  title: string;
  username: string;
  password: string;
  url: string;
  notes: string;
  tags: string[];
  expires: string | null;
  fields: FieldEdit[];
  totp: TotpEdit;
  /** a standard icon, 0–68; null leaves the icon as it is */
  icon: number | null;
}

export interface SaveOptions {
  /** overwrite although another program changed the file */
  force?: boolean;
  /** the user agreed to the file becoming KDBX 4.1 */
  upgrade?: boolean;
}

/** KeePass field keys, plus the pseudo-field for the current TOTP code. */
export const Field = {
  Title: 'Title',
  UserName: 'UserName',
  Password: 'Password',
  URL: 'URL',
  Notes: 'Notes',
  Totp: '@totp',
} as const;

export const ERROR_CODES = [
  'locked',
  'notFound',
  'emptyPassword',
  'noDatabase',
  'fileNotFound',
  'fileUnreadable',
  'keyFileUnreadable',
  'wrongKey',
  'corrupt',
  'unsupported',
  'keePass1',
  'notKeePass',
  'totpInvalid',
  'unsafeUrl',
  'clipboard',
  'notYet',
  'settingsNotSaved',
  'hotkeyInvalid',
  'hotkeyTaken',
  'autostart',
  'autotypeDisabled',
  'autotypeSequence',
  'autotypeField',
  'autotypeElevated',
  'autotypeNoWindow',
  'autotypeFailed',
  'window',
  'internal',
  'fieldName',
  'expiry',
  'unsavedChanges',
  'saveConflict',
  'saveFailed',
  'formatUpgrade',
  'notAllowed',
  'emptyName',
  'fileExists',
  'writeFailed',
  'tooLarge',
  'noCharacters',
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

export class AppError extends Error {
  readonly code: ErrorCode;
  constructor(code: ErrorCode) {
    super(code);
    this.code = code;
  }
}
