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
}

export interface AppInfo {
  version: string;
  effects: boolean;
  demo: boolean;
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
};

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
  'wrongKeyOrCorrupt',
  'corrupt',
  'unsupported',
  'keePass1',
  'notKeePass',
  'totpInvalid',
  'unsafeUrl',
  'clipboard',
  'notYet',
  'settingsNotSaved',
  'window',
  'internal',
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

export class AppError extends Error {
  readonly code: ErrorCode;
  constructor(code: ErrorCode) {
    super(code);
    this.code = code;
  }
}
