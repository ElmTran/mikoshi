import { useEffect, useLayoutEffect, useState, type RefObject } from 'react';

/**
 * A selection pill that slides between items instead of jumping (Fluent's
 * navigation indicator). Place the returned element inside `container`
 * (position: relative); it follows the element matching `selector`.
 */
export function useSlidingIndicator(
  container: RefObject<HTMLElement | null>,
  selector: string,
  deps: unknown[],
) {
  const [pos, setPos] = useState<{ y: number; h: number } | null>(null);
  // no slide on first placement, only when the selection moves
  const [ready, setReady] = useState(false);

  useLayoutEffect(() => {
    const el = container.current?.querySelector<HTMLElement>(selector);
    // measuring layout is what this effect is for
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setPos(el ? { y: el.offsetTop, h: el.offsetHeight } : null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  useEffect(() => {
    if (!pos || ready) return;
    const id = requestAnimationFrame(() => setReady(true));
    return () => cancelAnimationFrame(id);
  }, [pos, ready]);

  return pos ? (
    <span
      className={ready ? 'sel-indicator animate' : 'sel-indicator'}
      style={{ transform: `translateY(${pos.y + pos.h / 2 - 8}px)` }}
      aria-hidden="true"
    />
  ) : null;
}
