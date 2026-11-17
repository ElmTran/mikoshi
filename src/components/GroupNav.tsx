import type { RefObject } from 'react';
import { usePrefs } from '../app/contexts';
import type { GroupView } from '../api/types';
import { GroupIcon } from './bits';
import { Icon } from './Icon';
import { useSlidingIndicator } from './useSlidingIndicator';

interface Props {
  /** groups whose ancestors are all expanded, in tree order */
  visible: GroupView[];
  selected: string | null;
  collapsed: ReadonlySet<string>;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
  /** the tree takes keyboard focus as a whole; Workspace moves the selection */
  treeRef: RefObject<HTMLElement | null>;
}

export function GroupNav({ visible, selected, collapsed, onSelect, onToggle, treeRef }: Props) {
  const { t } = usePrefs();
  const indicator = useSlidingIndicator(treeRef, '.group.on', [selected, visible]);

  return (
    <nav className="groups pane" aria-label={t('groups.label')} role="tree" tabIndex={0} ref={treeRef}>
      {indicator}
      {visible.map((g) => {
        const open = !collapsed.has(g.id);
        return (
          <button
            key={g.id}
            type="button"
            tabIndex={-1}
            role="treeitem"
            aria-level={g.depth + 1}
            aria-selected={g.id === selected}
            aria-expanded={g.hasChildren ? open : undefined}
            className={[
              'group',
              g.kind === 'root' ? 'root' : '',
              g.kind === 'recycleBin' ? 'bin' : '',
              g.id === selected ? 'on' : '',
            ].join(' ')}
            style={{ ['--indent' as string]: Math.max(0, g.depth - 1) }}
            onClick={() => onSelect(g.id)}
            onDoubleClick={() => g.hasChildren && onToggle(g.id)}
          >
            <span
              className={g.hasChildren ? 'twisty' : 'twisty none'}
              title={g.hasChildren ? t(open ? 'groups.collapse' : 'groups.expand') : undefined}
              onClick={(e) => {
                if (!g.hasChildren) return;
                e.stopPropagation();
                onToggle(g.id);
              }}
            >
              {g.hasChildren && <Icon name="chevron" size={12} className={open ? undefined : 'closed'} />}
            </span>
            <GroupIcon icon={g.icon} customIcon={g.customIcon} />
            <span className="group-name">{g.name}</span>
            <span className="count">{g.entryCount}</span>
          </button>
        );
      })}
    </nav>
  );
}
