import { useRef } from 'react';
import { usePrefs } from '../app/contexts';
import type { GroupView } from '../api/types';
import { Icon } from './Icon';
import { useSlidingIndicator } from './useSlidingIndicator';

interface Props {
  groups: GroupView[];
  selected: string | null;
  onSelect: (id: string) => void;
}

export function GroupNav({ groups, selected, onSelect }: Props) {
  const { t } = usePrefs();
  const ref = useRef<HTMLElement>(null);
  const indicator = useSlidingIndicator(ref, '.group.on', [selected, groups]);

  return (
    <nav className="groups" aria-label={t('groups.label')} ref={ref}>
      {indicator}
      {groups.map((g) => (
        <button
          key={g.id}
          className={[
            'group',
            `depth-${Math.min(g.depth, 4)}`,
            g.kind === 'root' ? 'root' : '',
            g.kind === 'recycleBin' ? 'bin' : '',
            g.id === selected ? 'on' : '',
          ].join(' ')}
          aria-current={g.id === selected ? 'true' : undefined}
          onClick={() => onSelect(g.id)}
        >
          <Icon name={g.kind === 'root' ? 'vault' : g.kind === 'recycleBin' ? 'trash' : 'folder'} />
          <span className="group-name">{g.name}</span>
          <span className="count">{g.entryCount}</span>
        </button>
      ))}
    </nav>
  );
}
