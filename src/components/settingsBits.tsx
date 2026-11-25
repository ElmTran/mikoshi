import type { ReactNode } from 'react';

/** A setting that is on or off: the whole row is the button. */
export function Switch({
  on,
  label,
  hint,
  onToggle,
}: {
  on: boolean;
  label: string;
  hint?: string;
  onToggle: () => void;
}) {
  return (
    <button type="button" role="switch" aria-checked={on} className="switch-row" onClick={onToggle}>
      <span className="switch-text">
        <span>{label}</span>
        {hint && <span className="switch-hint">{hint}</span>}
      </span>
      <span className={on ? 'switch on' : 'switch'} aria-hidden="true" />
    </button>
  );
}

export function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string | undefined;
  children: ReactNode;
}) {
  return (
    <section className="settings-section">
      <h2 className="settings-section-title">{title}</h2>
      {hint && <p className="settings-section-hint">{hint}</p>}
      <div className="settings-rows">{children}</div>
    </section>
  );
}
