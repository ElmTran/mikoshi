import { useCallback, useMemo, useRef, useState } from 'react';
import type { EditableEntry, EntryEdit, FieldEdit, TotpEdit, TotpSummary } from '../api/types';
import { usePrefs } from '../app/contexts';
import { withShortcut } from '../shortcuts';
import { GeneratorPanel } from './Generator';
import { Icon } from './Icon';
import { IconPicker } from './IconPicker';
import { StrengthMeter } from './Strength';
import { useDismiss } from './useDismiss';

interface Props {
  /** the entry being edited; null for a new one */
  entry: EditableEntry | null;
  /** where a new entry goes */
  groupName: string;
  busy: boolean;
  onSave: (edit: EntryEdit) => void;
  onCancel: () => void;
}

interface FieldDraft {
  original: string | null;
  key: string;
  value: string;
  protected: boolean;
}

type TotpMode = 'keep' | 'remove' | 'uri' | 'manual';

interface Form {
  title: string;
  username: string;
  password: string;
  url: string;
  notes: string;
  /** as typed; split on , and ; when saving */
  tags: string;
  expiresOn: boolean;
  /** local time for the datetime input */
  expires: string;
  fields: FieldDraft[];
  totpMode: TotpMode;
  totpUri: string;
  totpSecret: string;
  totpAlgorithm: string;
  totpDigits: number;
  totpPeriod: number;
  /** a standard icon picked here; null leaves the icon as it is */
  icon: number | null;
}

/** UTC ISO to the value a `datetime-local` input takes (local time). */
function toLocalInput(iso: string | null): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** `datetime-local` value (local time) to UTC ISO, as the Rust side reads it. */
function fromLocalInput(value: string): string | null {
  if (!value) return null;
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? null : d.toISOString().replace(/\.\d+Z$/, 'Z');
}

function initial(entry: EditableEntry | null): Form {
  const expires = entry?.expires ?? null;
  return {
    title: entry?.title ?? '',
    username: entry?.username ?? '',
    password: entry?.password ?? '',
    url: entry?.url ?? '',
    notes: entry?.notes ?? '',
    tags: (entry?.tags ?? []).join(', '),
    expiresOn: expires !== null,
    expires: toLocalInput(expires),
    fields: (entry?.fields ?? []).map((f) => ({ original: f.key, ...f })),
    totpMode: 'keep',
    totpUri: '',
    totpSecret: '',
    totpAlgorithm: entry?.totp?.algorithm ?? 'SHA1',
    totpDigits: entry?.totp?.digits ?? 6,
    totpPeriod: entry?.totp?.period ?? 30,
    icon: null,
  };
}

function Summary({ totp }: { totp: TotpSummary }) {
  const { t } = usePrefs();
  if (!totp.valid) return <span className="muted">{t('editor.totp.invalid')}</span>;
  if (totp.steam) return <span>{t('editor.totp.steam')}</span>;
  return (
    <span>
      {t('editor.totp.summary', {
        algorithm: totp.algorithm ?? 'SHA1',
        digits: totp.digits ?? 6,
        period: totp.period ?? 30,
      })}
    </span>
  );
}

/** The key button next to the password field, with the generator in a popover. */
function GeneratorButton({ onUse }: { onUse: (password: string) => void }) {
  const { t, settings } = usePrefs();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, root, close);
  return (
    <div className="gen-anchor" ref={root}>
      <button
        type="button"
        className={open ? 'act active' : 'act'}
        title={withShortcut(t('editor.generate'), settings.shortcuts.generator)}
        aria-label={t('editor.generate')}
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        data-generator-toggle
      >
        <Icon name="key" />
      </button>
      {open && (
        <div className="flyout gen-flyout" role="dialog" aria-label={t('gen.title')}>
          <GeneratorPanel
            onUse={(pw) => {
              onUse(pw);
              setOpen(false);
            }}
          />
        </div>
      )}
    </div>
  );
}

/**
 * The edit form, shown in place of the detail pane. Values come in with the
 * entry and are dropped by the parent when editing ends (features.md §6).
 */
export function EntryEditor({ entry, groupName, busy, onSave, onCancel }: Props) {
  const { t } = usePrefs();
  const [form, setForm] = useState<Form>(() => initial(entry));
  const [showPassword, setShowPassword] = useState(false);
  const set = useCallback(<K extends keyof Form>(key: K, value: Form[K]) => {
    setForm((f) => ({ ...f, [key]: value }));
  }, []);

  const setField = useCallback((i: number, patch: Partial<FieldDraft>) => {
    setForm((f) => ({ ...f, fields: f.fields.map((x, j) => (j === i ? { ...x, ...patch } : x)) }));
  }, []);

  const totp: TotpEdit = useMemo(() => {
    switch (form.totpMode) {
      case 'keep':
        return { kind: 'unchanged' };
      case 'remove':
        return { kind: 'remove' };
      case 'uri':
        return { kind: 'uri', uri: form.totpUri };
      case 'manual':
        return {
          kind: 'manual',
          secret: form.totpSecret,
          algorithm: form.totpAlgorithm,
          digits: form.totpDigits,
          period: form.totpPeriod,
        };
    }
  }, [form]);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    const fields: FieldEdit[] = form.fields.map((f) => ({
      original: f.original,
      key: f.key.trim(),
      value: f.value,
      protected: f.protected,
    }));
    onSave({
      title: form.title,
      username: form.username,
      password: form.password,
      url: form.url,
      notes: form.notes,
      tags: form.tags
        .split(/[;,]/)
        .map((s) => s.trim())
        .filter(Boolean),
      expires: form.expiresOn ? fromLocalInput(form.expires) : null,
      fields,
      totp,
      icon: form.icon,
    });
  };

  const addingTotp = entry?.totp == null;
  const mode = addingTotp && form.totpMode === 'keep' ? 'none' : form.totpMode;

  return (
    <form className="detail-scroll editor" onSubmit={submit} data-editor>
      <header className="detail-head">
        <IconPicker
          icon={entry?.icon ?? 0}
          customIcon={entry?.customIcon ?? null}
          title={form.title}
          picked={form.icon}
          onPick={(i) => set('icon', i)}
        />
        <div className="detail-titles">
          <h1 className="detail-title">{entry ? t('editor.edit') : t('editor.new')}</h1>
          <div className="detail-path">{groupName}</div>
        </div>
        <div className="detail-actions">
          <button type="button" className="btn" onClick={onCancel} disabled={busy}>
            {t('editor.cancel')}
          </button>
          <button type="submit" className="btn primary" disabled={busy}>
            {busy ? t('editor.saving') : t('editor.save')}
          </button>
        </div>
      </header>

      <div className="editor-fields">
        <label className="ef">
          <span>{t('editor.title')}</span>
          <input
            value={form.title}
            onChange={(e) => set('title', e.target.value)}
            autoFocus={!entry}
            spellCheck={false}
          />
        </label>
        <label className="ef">
          <span>{t('editor.username')}</span>
          <input value={form.username} onChange={(e) => set('username', e.target.value)} spellCheck={false} />
        </label>
        <div className="ef">
          <span>{t('editor.password')}</span>
          <div className="ef-row ef-box">
            <input
              type={showPassword ? 'text' : 'password'}
              value={form.password}
              onChange={(e) => set('password', e.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <button
              type="button"
              className="act"
              title={t(showPassword ? 'editor.hide' : 'editor.show')}
              aria-pressed={showPassword}
              onClick={() => setShowPassword((s) => !s)}
            >
              <Icon name={showPassword ? 'eyeOff' : 'eye'} />
            </button>
            <GeneratorButton
              onUse={(pw) => {
                set('password', pw);
                setShowPassword(true);
              }}
            />
          </div>
          <StrengthMeter password={form.password} />
        </div>
        <label className="ef">
          <span>{t('editor.url')}</span>
          <input value={form.url} onChange={(e) => set('url', e.target.value)} spellCheck={false} />
        </label>
        <div className="ef-two">
          <label className="ef">
            <span>{t('editor.tags')}</span>
            <input
              value={form.tags}
              onChange={(e) => set('tags', e.target.value)}
              placeholder={t('editor.tagsHint')}
              spellCheck={false}
            />
          </label>
          <div className="ef">
            <span>{t('editor.expires')}</span>
            <div className="ef-row ef-box">
              <label className="check">
                <input
                  type="checkbox"
                  checked={form.expiresOn}
                  onChange={(e) =>
                    setForm((f) => ({
                      ...f,
                      expiresOn: e.target.checked,
                      expires:
                        e.target.checked && !f.expires ? toLocalInput(new Date().toISOString()) : f.expires,
                    }))
                  }
                />
                <span>{t('editor.expiresOn')}</span>
              </label>
              {form.expiresOn && (
                <input
                  type="datetime-local"
                  value={form.expires}
                  onChange={(e) => set('expires', e.target.value)}
                />
              )}
            </div>
          </div>
        </div>
        <label className="ef">
          <span>{t('editor.notes')}</span>
          <textarea
            rows={4}
            value={form.notes}
            onChange={(e) => set('notes', e.target.value)}
            spellCheck={false}
          />
        </label>
      </div>

      <section className="detail-section">
        <h2>{t('editor.fields')}</h2>
        <div className="field-rows">
          {form.fields.length === 0 && <div className="muted">{t('editor.totp.none')}</div>}
          {form.fields.map((f, i) => (
            <div className="field-row" key={f.original ?? `new-${i}`}>
              <input
                className="fr-key"
                value={f.key}
                placeholder={t('editor.fieldKey')}
                onChange={(e) => setField(i, { key: e.target.value })}
                spellCheck={false}
              />
              <input
                className="fr-value"
                type={f.protected && !showPassword ? 'password' : 'text'}
                value={f.value}
                placeholder={t('editor.fieldValue')}
                onChange={(e) => setField(i, { value: e.target.value })}
                spellCheck={false}
              />
              <label className="check" title={t('editor.fieldProtected')}>
                <input
                  type="checkbox"
                  checked={f.protected}
                  onChange={(e) => setField(i, { protected: e.target.checked })}
                />
                <span>{t('editor.fieldProtected')}</span>
              </label>
              <button
                type="button"
                className="act"
                title={t('editor.removeField')}
                aria-label={t('editor.removeField')}
                onClick={() => setForm((x) => ({ ...x, fields: x.fields.filter((_, j) => j !== i) }))}
              >
                <Icon name="trash" />
              </button>
            </div>
          ))}
          <button
            type="button"
            className="btn small"
            onClick={() =>
              setForm((x) => ({
                ...x,
                fields: [...x.fields, { original: null, key: '', value: '', protected: false }],
              }))
            }
          >
            <Icon name="plus" />
            {t('editor.addField')}
          </button>
        </div>
      </section>

      <section className="detail-section">
        <h2>{t('editor.totp')}</h2>
        <div className="totp-editor">
          <div className="totp-now">
            <span className="field-label">
              {entry?.totp ? <Summary totp={entry.totp} /> : t('editor.totp.none')}
            </span>
            <div className="seg" role="group">
              {entry?.totp && (
                <button
                  type="button"
                  className={mode === 'keep' ? 'on' : ''}
                  onClick={() => set('totpMode', 'keep')}
                >
                  {t('editor.totp.keep')}
                </button>
              )}
              <button
                type="button"
                className={mode === 'uri' || mode === 'manual' ? 'on' : ''}
                onClick={() => set('totpMode', mode === 'keep' ? 'uri' : mode === 'none' ? 'uri' : mode)}
              >
                {entry?.totp ? t('editor.totp.replace') : t('editor.totp.add')}
              </button>
              {entry?.totp && (
                <button
                  type="button"
                  className={mode === 'remove' ? 'on danger' : ''}
                  onClick={() => set('totpMode', 'remove')}
                >
                  {t('editor.totp.remove')}
                </button>
              )}
            </div>
          </div>

          {(mode === 'uri' || mode === 'manual') && (
            <div className="totp-new">
              <label className="ef">
                <span>{t('editor.totp.orPaste')}</span>
                <input
                  value={form.totpUri}
                  onChange={(e) => setForm((f) => ({ ...f, totpUri: e.target.value, totpMode: 'uri' }))}
                  placeholder="otpauth://totp/..."
                  spellCheck={false}
                  autoComplete="off"
                />
              </label>
              <div className="totp-or">{t('editor.totp.orManual')}</div>
              <label className="ef">
                <span>{t('editor.totp.orManual')}</span>
                <input
                  value={form.totpSecret}
                  onChange={(e) => setForm((f) => ({ ...f, totpSecret: e.target.value, totpMode: 'manual' }))}
                  spellCheck={false}
                  autoComplete="off"
                />
              </label>
              <div className="ef-row">
                <label className="ef small">
                  <span>{t('editor.totp.algorithm')}</span>
                  <select value={form.totpAlgorithm} onChange={(e) => set('totpAlgorithm', e.target.value)}>
                    <option value="SHA1">SHA-1</option>
                    <option value="SHA256">SHA-256</option>
                    <option value="SHA512">SHA-512</option>
                  </select>
                </label>
                <label className="ef small">
                  <span>{t('editor.totp.digits')}</span>
                  <input
                    type="number"
                    min={4}
                    max={10}
                    value={form.totpDigits}
                    onChange={(e) => set('totpDigits', Number(e.target.value))}
                  />
                </label>
                <label className="ef small">
                  <span>{t('editor.totp.period')}</span>
                  <input
                    type="number"
                    min={1}
                    value={form.totpPeriod}
                    onChange={(e) => set('totpPeriod', Number(e.target.value))}
                  />
                </label>
              </div>
            </div>
          )}
        </div>
      </section>
    </form>
  );
}
