import { useEffect, useState, type ReactNode } from 'react';
import { api } from '../api';
import { AppError, Field, type EntryDetail as Detail, type TotpView } from '../api/types';
import { usePrefs } from '../app/contexts';
import { EntryIcon, Keys } from './bits';
import { Icon } from './Icon';

export interface DetailActions {
  copy: (key: string) => void;
  toggleReveal: (key: string) => void;
  openUrl: () => void;
  autotype: () => void;
  edit: () => void;
  exportAttachment: (name: string) => void;
}

interface Props {
  entry: Detail;
  /** values the user chose to show, by field key; dropped on entry change */
  revealed: Record<string, string>;
  /** field key whose copy button shows the "copied" check */
  copied: string | null;
  actions: DetailActions;
}

const MASK = '••••••••••••'; // fixed length: never hints at the real one

function Totp({ entryId }: { entryId: string }) {
  const { t } = usePrefs();
  const [totp, setTotp] = useState<TotpView | null>(null);
  // the entry has TOTP settings that cannot be used
  const [invalid, setInvalid] = useState(false);

  useEffect(() => {
    let live = true;
    let timer = 0;
    const load = () =>
      api.getTotp(entryId).then(
        (v) => {
          if (!live) return;
          setTotp(v);
          // the code is computed on the Rust side; refetch when it rolls over
          timer = window.setTimeout(load, v.remaining * 1000 + 50);
        },
        (e: unknown) => {
          if (!live) return;
          setTotp(null);
          setInvalid(e instanceof AppError && e.code === 'totpInvalid');
        },
      );
    void load();
    const tick = window.setInterval(
      () => setTotp((v) => (v ? { ...v, remaining: Math.max(0, v.remaining - 1) } : v)),
      1000,
    );
    return () => {
      live = false;
      window.clearTimeout(timer);
      window.clearInterval(tick);
    };
  }, [entryId]);

  if (!totp) return <span className="muted">{invalid ? t('totp.invalid') : '······'}</span>;
  const low = totp.remaining <= 5;
  return (
    <span className={low ? 'totp low' : 'totp'}>
      <span className="totp-code">
        {totp.code.length >= 6
          ? `${totp.code.slice(0, totp.code.length / 2)} ${totp.code.slice(totp.code.length / 2)}`
          : totp.code}
      </span>
      <span className="ring" style={{ ['--p' as string]: totp.remaining / totp.period }} />
      <span className="totp-left">{t('totp.remaining', { n: totp.remaining })}</span>
    </span>
  );
}

interface RowProps {
  label: string;
  fieldKey: string;
  value: ReactNode;
  shortcut?: string[];
  secret?: boolean;
  shown?: boolean;
  revealShortcut?: string[];
  open?: boolean;
  /** no value: shows a dash, and there is nothing to copy */
  empty?: boolean;
  copied: boolean;
  actions: DetailActions;
}

function FieldRow({
  label,
  fieldKey,
  value,
  shortcut,
  secret,
  shown,
  revealShortcut,
  open,
  empty,
  copied,
  actions,
}: RowProps) {
  const { t } = usePrefs();
  const hint = (name: string, keys?: string[]) => (keys ? `${name} (${keys.join('+')})` : name);
  return (
    <div className="field">
      <div className="field-label">{label}</div>
      <div className="field-value">{empty ? <span className="muted">—</span> : value}</div>
      <div className="field-actions">
        {shortcut && !empty && <Keys keys={shortcut} />}
        {open && (
          <button
            className="act"
            title={hint(t('field.open'), ['Ctrl', 'Shift', 'U'])}
            onClick={actions.openUrl}
          >
            <Icon name="open" />
          </button>
        )}
        {secret && (
          <button
            className="act"
            title={hint(t(shown ? 'field.hide' : 'field.show'), revealShortcut)}
            aria-pressed={shown}
            onClick={() => actions.toggleReveal(fieldKey)}
          >
            <Icon name={shown ? 'eyeOff' : 'eye'} />
          </button>
        )}
        {!empty && (
          <button
            className={copied ? 'act copy done' : 'act copy'}
            title={hint(t('field.copy'), shortcut)}
            onClick={() => actions.copy(fieldKey)}
          >
            <Icon name={copied ? 'check' : 'copy'} />
          </button>
        )}
      </div>
    </div>
  );
}

export function EntryDetail({ entry, revealed, copied, actions }: Props) {
  const { t, fmt } = usePrefs();
  const secretValue = (key: string) =>
    key in revealed ? (
      <span className="secret-plain">{revealed[key]}</span>
    ) : (
      <span className="mask">{MASK}</span>
    );

  return (
    <div className="detail-scroll" key={entry.id}>
      <header className="detail-head">
        <EntryIcon icon={entry.icon} customIcon={entry.customIcon} title={entry.title} size="lg" />
        <div className="detail-titles">
          <h1 className="detail-title">{entry.title}</h1>
          <div className="detail-path">{entry.groupPath.join(' / ')}</div>
        </div>
        <div className="detail-actions">
          <button
            className="btn primary"
            title={`${t('detail.autotype')} (Ctrl+Shift+V)`}
            onClick={actions.autotype}
          >
            <Icon name="keyboard" />
            {t('detail.autotype')}
          </button>
          <button className="btn" title={`${t('detail.edit')} (Ctrl+E)`} onClick={actions.edit}>
            <Icon name="edit" />
            {t('detail.edit')}
          </button>
        </div>
      </header>

      {entry.expired && entry.expires && (
        <div className="infobar danger">
          <Icon name="warn" />
          <span>{t('detail.expiredNote', { date: fmt.date(entry.expires) })}</span>
        </div>
      )}

      {entry.tags.length > 0 && (
        <div className="tags">
          {entry.tags.map((tag) => (
            <span className="tag" key={tag}>
              <Icon name="tag" size={12} />
              {tag}
            </span>
          ))}
        </div>
      )}

      <div className="card fields">
        <FieldRow
          label={t('field.username')}
          fieldKey={Field.UserName}
          value={entry.username}
          empty={!entry.username}
          shortcut={['Ctrl', 'B']}
          copied={copied === Field.UserName}
          actions={actions}
        />
        {entry.hasPassword && (
          <FieldRow
            label={t('field.password')}
            fieldKey={Field.Password}
            value={secretValue(Field.Password)}
            shortcut={['Ctrl', 'C']}
            secret
            shown={Field.Password in revealed}
            revealShortcut={['Ctrl', 'H']}
            copied={copied === Field.Password}
            actions={actions}
          />
        )}
        {entry.url && (
          <FieldRow
            label={t('field.url')}
            fieldKey={Field.URL}
            value={<span className="link">{entry.url}</span>}
            shortcut={['Ctrl', 'U']}
            open
            copied={copied === Field.URL}
            actions={actions}
          />
        )}
        {entry.hasTotp && (
          <FieldRow
            label={t('field.totp')}
            fieldKey={Field.Totp}
            value={<Totp entryId={entry.id} />}
            shortcut={['Ctrl', 'T']}
            copied={copied === Field.Totp}
            actions={actions}
          />
        )}
        {entry.fields.map((f) => (
          <FieldRow
            key={f.key}
            label={f.key}
            fieldKey={f.key}
            value={f.protected ? secretValue(f.key) : f.value}
            // a protected value is unknown until shown, so only plain ones can be empty
            empty={!f.protected && !f.value}
            secret={f.protected}
            shown={f.key in revealed}
            copied={copied === f.key}
            actions={actions}
          />
        ))}
      </div>

      {entry.notes && (
        <section className="detail-section">
          <h2>{t('detail.notes')}</h2>
          <p className="notes">{entry.notes}</p>
        </section>
      )}

      {entry.attachments.length > 0 && (
        <section className="detail-section">
          <h2>{t('detail.attachments')}</h2>
          <ul className="attachments">
            {entry.attachments.map((a) => (
              <li key={a.name} className="card">
                <Icon name="clip" />
                <span className="att-name">{a.name}</span>
                <span className="att-size">{fmt.size(a.size)}</span>
                <button
                  className="act"
                  title={t('detail.export')}
                  onClick={() => actions.exportAttachment(a.name)}
                >
                  <Icon name="download" />
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      <footer className="times">
        <div>
          <span>{t('detail.created')}</span>
          <span>{fmt.dateTime(entry.created)}</span>
        </div>
        <div>
          <span>{t('detail.modified')}</span>
          <span>{fmt.dateTime(entry.modified)}</span>
        </div>
        <div className={entry.expired ? 'bad' : undefined}>
          <span>{t('detail.expires')}</span>
          <span>{entry.expires ? fmt.dateTime(entry.expires) : t('detail.never')}</span>
        </div>
      </footer>
    </div>
  );
}
