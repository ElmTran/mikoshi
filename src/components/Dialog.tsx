import { useEffect, useRef, type ReactNode } from 'react';

export interface DialogAction {
  label: string;
  onClick: () => void;
  /** the recommended action; Enter picks it */
  primary?: boolean;
  danger?: boolean;
}

interface Props {
  title: string;
  children?: ReactNode;
  actions: DialogAction[];
  /** Esc, the backdrop, or the × button */
  onClose: () => void;
}

/**
 * A modal over the whole window: a sheet with a title, the message and a row
 * of actions. Enter runs the primary action, Esc closes.
 */
export function Dialog({ title, children, actions, onClose }: Props) {
  const card = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // focus the primary action so Enter works right away
    card.current?.querySelector<HTMLButtonElement>('.dialog-actions .primary')?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      } else if (e.key === 'Enter' && !(e.target instanceof HTMLTextAreaElement)) {
        const primary = card.current?.querySelector<HTMLButtonElement>('.dialog-actions .primary');
        if (primary && document.activeElement !== document.querySelector('[data-dialog-text]')) {
          e.preventDefault();
          primary.click();
        }
      }
    };
    // capture: the workspace's shortcuts must not see these keys
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog" role="dialog" aria-modal="true" aria-label={title} ref={card}>
        <h2 className="dialog-title">{title}</h2>
        {children && <div className="dialog-body">{children}</div>}
        <div className="dialog-actions">
          {actions.map((a) => (
            <button
              key={a.label}
              className={`btn${a.primary ? ' primary' : ''}${a.danger ? ' danger' : ''}`}
              onClick={a.onClick}
            >
              {a.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
