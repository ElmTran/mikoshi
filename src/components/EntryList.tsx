import { useEffect, useRef } from 'react';
import { usePrefs } from '../app/contexts';
import type { EntrySummary } from '../api/types';
import { Avatar } from './bits';
import { Icon } from './Icon';
import { useSlidingIndicator } from './useSlidingIndicator';

interface Props {
  title: string;
  entries: EntrySummary[];
  selected: string | null;
  query: string;
  onSelect: (id: string) => void;
  onNew: () => void;
}

function Highlight({ text, query }: { text: string; query: string }) {
  const q = query.trim();
  const i = q ? text.toLowerCase().indexOf(q.toLowerCase()) : -1;
  if (i < 0) return <>{text}</>;
  return (
    <>
      {text.slice(0, i)}
      <mark>{text.slice(i, i + q.length)}</mark>
      {text.slice(i + q.length)}
    </>
  );
}

export function EntryList({ title, entries, selected, query, onSelect, onNew }: Props) {
  const { t, tn, fmt } = usePrefs();
  const rows = useRef<HTMLDivElement>(null);
  const indicator = useSlidingIndicator(rows, '.row.on', [selected, entries]);

  useEffect(() => {
    rows.current?.querySelector('.row.on')?.scrollIntoView({ block: 'nearest' });
  }, [selected]);

  return (
    <section className="list" aria-label={t('list.label')}>
      <div className="list-head">
        <h2 className="list-title">{title}</h2>
        <span className="list-count">{tn('list.count', entries.length)}</span>
        <button
          className="icon-btn"
          title={`${t('list.new')} (Ctrl+N)`}
          aria-label={t('list.new')}
          onClick={onNew}
        >
          <Icon name="plus" />
        </button>
      </div>
      <div className="rows" role="listbox" aria-label={title} ref={rows}>
        {indicator}
        {entries.map((e) => (
          <div
            key={e.id}
            className={e.id === selected ? 'row on' : 'row'}
            role="option"
            aria-selected={e.id === selected}
            onClick={() => onSelect(e.id)}
          >
            <Avatar title={e.title} />
            <span className="row-title">
              <span className="row-title-text">
                <Highlight text={e.title} query={query} />
              </span>
              {e.expired && <span className="badge danger">{t('list.expired')}</span>}
            </span>
            <span className="row-user">
              <Highlight text={e.username} query={query} />
            </span>
            <span className="row-date">{fmt.date(e.modified)}</span>
          </div>
        ))}
        {entries.length === 0 && (
          <div className="empty">{query ? t('search.none', { q: query }) : t('list.empty')}</div>
        )}
      </div>
    </section>
  );
}
