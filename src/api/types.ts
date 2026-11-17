// Mirrors the Rust view types in crates/mikoshi-core/src/model.rs and the
// command results in src-tauri/src/commands.rs. None of these ever carries a
// password, a protected field value or a TOTP secret.

export type GroupKind = 'root' | 'normal' | 'recycleBin';

export interface GroupView {
  id: string;
  name: string;
  parentId: string | null;
  depth: number;
  kind: GroupKind;
  entryCount: number;
}

export interface EntrySummary {
  id: string;
  groupId: string;
  title: string;
  username: string;
  url: string;
  modified: string;
  expired: boolean;
  hasTotp: boolean;
  tags: string[];
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
  created: string;
  modified: string;
  expires: string | null;
  expired: boolean;
  hasPassword: boolean;
  hasTotp: boolean;
  fields: FieldView[];
  attachments: AttachmentView[];
}

export interface TotpView {
  code: string;
  period: number;
  remaining: number;
}

export interface VaultStatus {
  locked: boolean;
  name: string;
  file: string;
  path: string;
}

export interface AppInfo {
  version: string;
  effects: boolean;
  demo: boolean;
}

export interface CopyResult {
  clearsAfter: number | null;
}

export type ThemePref = 'system' | 'light' | 'dark';
export type LanguagePref = 'system' | 'zh-CN' | 'en-US';

export interface Settings {
  theme: ThemePref;
  language: LanguagePref;
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

export type ErrorCode = 'locked' | 'notFound' | 'emptyPassword' | 'settingsNotSaved' | 'window' | 'internal';

export class AppError extends Error {
  readonly code: ErrorCode;
  constructor(code: ErrorCode) {
    super(code);
    this.code = code;
  }
}
