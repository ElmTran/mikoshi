import { useEffect, useRef, useState, type DragEvent, type MouseEvent, type RefObject } from 'react';
import { usePrefs } from '../app/contexts';
import type { GroupView } from '../api/types';
import { GroupIcon } from './bits';
import { Icon } from './Icon';
import { useSlidingIndicator } from './useSlidingIndicator';

/** What is being dragged around the window. */
export type Dragged = { kind: 'entry'; ids: string[] } | { kind: 'group'; id: string };

interface Props {
  /** groups whose ancestors are all expanded, in tree order */
  visible: GroupView[];
  selected: string | null;
  collapsed: ReadonlySet<string>;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
  /** the tree takes keyboard focus as a whole; Workspace moves the selection */
  treeRef: RefObject<HTMLElement | null>;
  onContextMenu: (group: GroupView, e: MouseEvent) => void;
  /** the group whose name is being edited in place */
  renaming: string | null;
  /** `null`: renaming was cancelled */
  onRenamed: (id: string, name: string | null) => void;
  dragged: RefObject<Dragged | null>;
  onDragGroup: (id: string | null) => void;
  canDrop: (target: GroupView) => boolean;
  onDrop: (target: GroupView) => void;
}

function RenameBox({ name, onDone }: { name: string; onDone: (name: string | null) => void }) {
  const [value, setValue] = useState(name);
  const input = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useEffect(() => input.current?.select(), []);
  const finish = (v: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(v);
  };
  return (
    <input
      ref={input}
      className="group-rename"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Enter') finish(value);
        else if (e.key === 'Escape') finish(null);
      }}
      onBlur={() => finish(value)}
      spellCheck={false}
    />
  );
}

export function GroupNav({
  visible,
  selected,
  collapsed,
  onSelect,
  onToggle,
  treeRef,
  onContextMenu,
  renaming,
  onRenamed,
  dragged,
  onDragGroup,
  canDrop,
  onDrop,
}: Props) {
  const { t } = usePrefs();
  const indicator = useSlidingIndicator(treeRef, '.group.on', [selected, visible]);
  const [over, setOver] = useState<string | null>(null);

  const dragOver = (g: GroupView, e: DragEvent) => {
    if (!dragged.current || !canDrop(g)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    if (over !== g.id) setOver(g.id);
  };

  return (
    <nav className="groups pane" aria-label={t('groups.label')} role="tree" tabIndex={0} ref={treeRef}>
      {indicator}
      {visible.map((g) => {
        const open = !collapsed.has(g.id);
        const editing = renaming === g.id;
        const classes = [
          'group',
          g.kind === 'root' ? 'root' : '',
          g.kind === 'recycleBin' ? 'bin' : '',
          g.id === selected ? 'on' : '',
          over === g.id ? 'drop' : '',
        ].join(' ');
        if (editing) {
          // an input inside a button would hand Space and clicks to the button
          return (
            <div
              key={g.id}
              role="treeitem"
              aria-level={g.depth + 1}
              aria-selected={g.id === selected}
              className={`${classes} editing`}
              style={{ ['--indent' as string]: Math.max(0, g.depth - 1) }}
            >
              <span className="twisty none" />
              <GroupIcon icon={g.icon} customIcon={g.customIcon} />
              <RenameBox name={g.name} onDone={(name) => onRenamed(g.id, name)} />
            </div>
          );
        }
        return (
          <button
            key={g.id}
            type="button"
            tabIndex={-1}
            role="treeitem"
            aria-level={g.depth + 1}
            aria-selected={g.id === selected}
            aria-expanded={g.hasChildren ? open : undefined}
            className={classes}
            style={{ ['--indent' as string]: Math.max(0, g.depth - 1) }}
            onClick={() => onSelect(g.id)}
            onDoubleClick={() => g.hasChildren && onToggle(g.id)}
            onContextMenu={(e) => {
              e.preventDefault();
              onSelect(g.id);
              onContextMenu(g, e);
            }}
            draggable={g.kind === 'normal'}
            onDragStart={(e) => {
              e.dataTransfer.effectAllowed = 'move';
              // some browsers need data for the drag to start at all
              e.dataTransfer.setData('text/plain', '');
              onDragGroup(g.id);
            }}
            onDragEnd={() => {
              onDragGroup(null);
              setOver(null);
            }}
            onDragOver={(e) => dragOver(g, e)}
            onDragLeave={() => over === g.id && setOver(null)}
            onDrop={(e) => {
              e.preventDefault();
              setOver(null);
              if (canDrop(g)) onDrop(g);
            }}
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
