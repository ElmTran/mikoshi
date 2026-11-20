import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent, type RefObject } from 'react';
import { usePrefs } from '../app/contexts';
import { withShortcut } from '../shortcuts';
import type { EntrySort, EntrySummary, SortKey } from '../api/types';
import { EntryIcon } from './bits';
import { Icon } from './Icon';
import { useDismiss } from './useDismiss';
import { useSlidingIndicator } from './useSlidingIndicator';
import { host, searchTerms } from '../text';
import { Highlight } from './Highlight';

const SORT_KEYS: SortKey[] = ['title', 'username', 'url', 'modified'];

interface Props {
  title: string;
  entries: EntrySummary[];
  selected: string | null;
  query: string;
  sort: EntrySort;
  onSort: (sort: EntrySort) => void;
  onSelect: (id: string) => void;
  onNew: () => void;
  /** the list takes keyboard focus as a whole; Workspace moves the selection */
  listRef: RefObject<HTMLElement | null>;
  onRowMenu: (entry: EntrySummary, e: MouseEvent) => void;
  onDragEntry: (id: string | null) => void;
  /** shown when the recycle bin is open: empties it */
  onEmptyBin?: (() => void) | undefined;
}

/** "Sort by" button and menu. Picking the current key again flips the order. */
function SortMenu({ sort, onSort }: { sort: EntrySort; onSort: (s: EntrySort) => void }) {
  const { t } = usePrefs();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, root, close);
  // like any menu: picking an item closes it
  const pick = (next: EntrySort) => {
    setOpen(false);
    onSort(next);
  };

  return (
    <div className="sort-anchor" ref={root}>
      <button
        type="button"
        className={open ? 'sort-btn active' : 'sort-btn'}
        aria-haspopup="menu"
        aria-expanded={open}
        title={t('list.sort')}
        onClick={() => setOpen((o) => !o)}
      >
        <span>{t(`list.sort.${sort.key}`)}</span>
        <Icon name="sortDown" size={14} className={sort.desc ? undefined : 'flip'} />
      </button>
      {open && (
        <div className="flyout sort-menu" role="menu" aria-label={t('list.sort')}>
          {SORT_KEYS.map((key) => (
            <button
              key={key}
              type="button"
              role="menuitemradio"
              aria-checked={sort.key === key}
              className={sort.key === key ? 'menu-item on' : 'menu-item'}
              onClick={() =>
                pick(sort.key === key ? { key, desc: !sort.desc } : { key, desc: key === 'modified' })
              }
            >
              <span>{t(`list.sort.${key}`)}</span>
              {sort.key === key && (
                <Icon name="sortDown" size={14} className={sort.desc ? undefined : 'flip'} />
              )}
            </button>
          ))}
          <div className="menu-sep" />
          {([false, true] as const).map((desc) => (
            <button
              key={String(desc)}
              type="button"
              role="menuitemradio"
              aria-checked={sort.desc === desc}
              className={sort.desc === desc ? 'menu-item on' : 'menu-item'}
              onClick={() => pick({ ...sort, desc })}
            >
              <span>{t(desc ? 'list.sort.desc' : 'list.sort.asc')}</span>
              {sort.desc === desc && <Icon name="check" size={14} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function EntryList({
  title,
  entries,
  selected,
  query,
  sort,
  onSort,
  onSelect,
  onNew,
  listRef,
  onRowMenu,
  onDragEntry,
  onEmptyBin,
}: Props) {
  const { t, tn, fmt, settings } = usePrefs();
  const terms = useMemo(() => searchTerms(query), [query]);
  const indicator = useSlidingIndicator(listRef, '.row.on', [selected, entries]);

  useEffect(() => {
    listRef.current?.querySelector('.row.on')?.scrollIntoView({ block: 'nearest' });
  }, [selected, listRef]);

  return (
    <section className="list" aria-label={t('list.label')}>
      <div className="list-head">
        <h2 className="list-title">{title}</h2>
        {onEmptyBin ? (
          <button
            className="btn small danger"
            title={t('menu.emptyBin')}
            onClick={onEmptyBin}
            disabled={entries.length === 0}
          >
            <Icon name="trash" />
            {t('menu.emptyBin')}
          </button>
        ) : (
          <button
            className="icon-btn"
            title={withShortcut(t('list.new'), settings.shortcuts.newEntry)}
            aria-label={t('list.new')}
            onClick={onNew}
          >
            <Icon name="plus" />
          </button>
        )}
      </div>
      <div className="list-sub">
        <span className="list-count">{tn('list.count', entries.length)}</span>
        {query ? (
          <span className="list-rank">{t('list.sortedByRank')}</span>
        ) : (
          <SortMenu sort={sort} onSort={onSort} />
        )}
      </div>
      <div
        className="rows pane"
        role="listbox"
        aria-label={title}
        tabIndex={0}
        ref={listRef as RefObject<HTMLDivElement>}
      >
        {indicator}
        {entries.map((e) => {
          const site = e.url ? host(e.url) : '';
          return (
            <div
              key={e.id}
              className={e.id === selected ? 'row on' : 'row'}
              role="option"
              aria-selected={e.id === selected}
              onClick={() => onSelect(e.id)}
              onContextMenu={(ev) => {
                ev.preventDefault();
                onSelect(e.id);
                onRowMenu(e, ev);
              }}
              draggable
              onDragStart={(ev) => {
                ev.dataTransfer.effectAllowed = 'move';
                ev.dataTransfer.setData('text/plain', '');
                onDragEntry(e.id);
              }}
              onDragEnd={() => onDragEntry(null)}
            >
              <EntryIcon icon={e.icon} customIcon={e.customIcon} title={e.title} />
              <span className="row-title">
                <span className="row-title-text">
                  <Highlight text={e.title} terms={terms} />
                </span>
                {e.expired && <span className="badge danger">{t('list.expired')}</span>}
              </span>
              <span className="row-user">
                <Highlight text={e.username} terms={terms} />
                {e.username && site && <span className="row-sep"> · </span>}
                {site && (
                  <span className="row-host">
                    <Highlight text={site} terms={terms} />
                  </span>
                )}
              </span>
              <span className="row-date">{fmt.date(e.modified)}</span>
            </div>
          );
        })}
        {entries.length === 0 && (
          <div className="empty">{query ? t('search.none', { q: query }) : t('list.empty')}</div>
        )}
      </div>
    </section>
  );
}
