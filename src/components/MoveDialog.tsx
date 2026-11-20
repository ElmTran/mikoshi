import { useState } from 'react';
import type { GroupView } from '../api/types';
import { usePrefs } from '../app/contexts';
import { GroupIcon } from './bits';
import { Dialog } from './Dialog';

interface Props {
  groups: GroupView[];
  /** groups that cannot be picked (the item's own group, a group's subtree) */
  excluded: ReadonlySet<string>;
  onMove: (to: string) => void;
  onClose: () => void;
}

/** "Move to…": every group outside the recycle bin, as a tree. */
export function MoveDialog({ groups, excluded, onMove, onClose }: Props) {
  const { t } = usePrefs();
  // the bin and everything in it are not targets: moving there is deleting
  const bin = groups.find((g) => g.kind === 'recycleBin');
  const inBin = new Set<string>();
  if (bin) {
    inBin.add(bin.id);
    for (const g of groups) if (g.parentId && inBin.has(g.parentId)) inBin.add(g.id);
  }
  const targets = groups.filter((g) => !inBin.has(g.id));
  const [picked, setPicked] = useState<string | null>(null);

  return (
    <Dialog
      title={t('move.title')}
      onClose={onClose}
      actions={[
        { label: t('editor.cancel'), onClick: onClose },
        {
          label: t('move.ok'),
          primary: true,
          onClick: () => picked && onMove(picked),
        },
      ]}
    >
      <div className="move-list" role="listbox" aria-label={t('move.title')}>
        {targets.map((g) => {
          const off = excluded.has(g.id);
          return (
            <button
              key={g.id}
              type="button"
              role="option"
              aria-selected={picked === g.id}
              disabled={off}
              className={picked === g.id ? 'move-item on' : 'move-item'}
              style={{ ['--indent' as string]: g.depth }}
              onClick={() => setPicked(g.id)}
              onDoubleClick={() => !off && onMove(g.id)}
            >
              <GroupIcon icon={g.icon} customIcon={g.customIcon} />
              <span>{g.name}</span>
            </button>
          );
        })}
      </div>
    </Dialog>
  );
}
