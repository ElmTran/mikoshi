import { useEffect, useState } from 'react';
import { api } from '../api';
import type { Strength } from '../api/types';
import { usePrefs } from '../app/contexts';

/** Five bars and a word, from zxcvbn's 0–4 score. */
export function StrengthBars({ strength }: { strength: Strength | null }) {
  const { t } = usePrefs();
  if (!strength) return null;
  const score = Math.max(0, Math.min(4, strength.score));
  return (
    <span className={`strength s${score}`} aria-label={t(`strength.${score}` as 'strength.0')}>
      <span className="strength-bars" aria-hidden="true">
        {[0, 1, 2, 3, 4].map((i) => (
          <span key={i} className={i <= score ? 'on' : undefined} />
        ))}
      </span>
      <span className="strength-word">{t(`strength.${score}` as 'strength.0')}</span>
    </span>
  );
}

/**
 * Strength of a password being typed. The estimate runs on the Rust side
 * (zxcvbn), a moment after typing stops.
 */
export function StrengthMeter({ password }: { password: string }) {
  const [strength, setStrength] = useState<Strength | null>(null);
  useEffect(() => {
    if (!password) return;
    let live = true;
    const timer = window.setTimeout(() => {
      api.passwordStrength(password).then(
        (s) => live && setStrength(s),
        () => {},
      );
    }, 150);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [password]);
  return <StrengthBars strength={password ? strength : null} />;
}
