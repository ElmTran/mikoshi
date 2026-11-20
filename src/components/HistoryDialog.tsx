import { useEffect, useState } from 'react';
import { api } from '../api';
import type { EntryDetail, HistoryItem } from '../api/types';
import { usePrefs } from '../app/contexts';
import { Icon } from './Icon';

interface Props {
  entryId: string;
  onClose: () => void;
  /** a version was made current; the caller refreshes the views */
  onRestored: () => void;
  onError: (e: unknown) => void;
}

const MASK = '••••••••••••';

/** Older versions of an entry, read-only, with a way back to any of them. */
export function HistoryDialog({ entryId, onClose, onRestored, onError }: Props) {
  const { t, fmt } = usePrefs();
  const [items, setItems] = useState<HistoryItem[] | null>(null);
  const [picked, setPicked] = useState<number | null>(null);
  const [view, setView] = useState<EntryDetail | null>(null);
  const [password, setPassword] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    api.listHistory(entryId).then(
      (list) => {
        if (!live) return;
        setItems(list);
        // newest first is the useful default
        setPicked(list.at(-1)?.index ?? null);
      },
      (e) => live && onError(e),
    );
    return () => {
      live = false;
    };
  }, [entryId, onError]);

  useEffect(() => {
    if (picked === null) return;
    let live = true;
    api.getHistoryEntry(entryId, picked).then(
      (d) => live && setView(d),
      (e) => live && onError(e),
    );
    return () => {
      live = false;
    };
  }, [entryId, picked, onError]);

  const restore = () => {
    if (picked === null || busy) return;
    setBusy(true);
    api.restoreHistory(entryId, picked).then(
      () => onRestored(),
      (e) => {
        setBusy(false);
        onError(e);
      },
    );
  };

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog card history" role="dialog" aria-modal="true" aria-label={t('history.title')}>
        <h2 className="dialog-title">{t('history.title')}</h2>
        {items && items.length === 0 ? (
          <div className="dialog-body muted">{t('history.empty')}</div>
        ) : (
          <div className="history-body">
            <ol className="history-list">
              {!items && <li className="muted">{t('history.loading')}</li>}
              {/* newest first */}
              {items
                ?.slice()
                .reverse()
                .map((h) => (
                  <li key={h.index}>
                    <button
                      className={picked === h.index ? 'on' : ''}
                      onClick={() => {
                        setPicked(h.index);
                        // a revealed password belongs to the version it was read from
                        setPassword(null);
                        setView(null);
                      }}
                    >
                      <span className="h-title">{h.title || '—'}</span>
                      <span className="h-date">{fmt.dateTime(h.modified)}</span>
                    </button>
                  </li>
                ))}
            </ol>
            <div className="history-view">
              {view ? (
                <>
                  <div className="hv-head">
                    <h3 className="detail-title">{view.title}</h3>
                    <div className="detail-path">{fmt.dateTime(view.modified)}</div>
                  </div>
                  <div className="card fields">
                    <div className="field">
                      <div className="field-label">{t('field.username')}</div>
                      <div className="field-value">{view.username || '—'}</div>
                    </div>
                    <div className="field">
                      <div className="field-label">{t('field.password')}</div>
                      <div className="field-value">
                        {password !== null ? (
                          <span className="secret-plain">{password}</span>
                        ) : (
                          <span className="mask">{MASK}</span>
                        )}
                      </div>
                      <div className="field-actions">
                        <button
                          className="act"
                          title={t(password === null ? 'field.show' : 'field.hide')}
                          aria-pressed={password !== null}
                          onClick={() => {
                            if (password !== null || picked === null) return setPassword(null);
                            api.revealHistoryField(entryId, picked, 'Password').then(setPassword, onError);
                          }}
                        >
                          <Icon name={password === null ? 'eye' : 'eyeOff'} />
                        </button>
                      </div>
                    </div>
                    {view.url && (
                      <div className="field">
                        <div className="field-label">{t('field.url')}</div>
                        <div className="field-value link">{view.url}</div>
                      </div>
                    )}
                    {view.fields.map((f) => (
                      <div className="field" key={f.key}>
                        <div className="field-label">{f.key}</div>
                        <div className="field-value">
                          {f.protected ? <span className="mask">{MASK}</span> : f.value || '—'}
                        </div>
                      </div>
                    ))}
                  </div>
                  {view.notes && <p className="notes">{view.notes}</p>}
                </>
              ) : (
                <div className="muted">{t('history.loading')}</div>
              )}
            </div>
          </div>
        )}
        <p className="dialog-hint">{t('history.hint')}</p>
        <div className="dialog-actions">
          <button className="btn" onClick={onClose}>
            {t('editor.cancel')}
          </button>
          <button className="btn primary" onClick={restore} disabled={picked === null || busy}>
            {t('history.restore')}
          </button>
        </div>
      </div>
    </div>
  );
}
