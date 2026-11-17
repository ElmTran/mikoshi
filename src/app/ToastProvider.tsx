import { useCallback, useRef, useState, type ReactNode } from 'react';
import { Icon } from '../components/Icon';
import { ToastContext, type ToastOptions } from './contexts';

interface Shown extends ToastOptions {
  id: number;
}

/** One toast at a time, bottom centre. Clipboard toasts drain over the clear time. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toast, setToast] = useState<Shown | null>(null);
  const timer = useRef(0);
  const seq = useRef(0);

  const show = useCallback((t: ToastOptions) => {
    seq.current += 1;
    const id = seq.current;
    setToast({ ...t, id });
    window.clearTimeout(timer.current);
    const ms = t.clearsAfter ? t.clearsAfter * 1000 : 2800;
    timer.current = window.setTimeout(() => setToast((cur) => (cur?.id === id ? null : cur)), ms);
  }, []);

  return (
    <ToastContext.Provider value={show}>
      {children}
      <div className="toast-host" role="status" aria-live="polite">
        {toast && (
          <div className={`toast tone-${toast.tone ?? 'success'}`} key={toast.id}>
            <span className="toast-icon">
              <Icon name={toast.tone === 'error' ? 'warn' : 'check'} size={12} />
            </span>
            <span className="toast-msg">{toast.message}</span>
            {toast.clearsAfter ? (
              <span className="toast-bar" style={{ animationDuration: `${toast.clearsAfter}s` }} />
            ) : null}
          </div>
        )}
      </div>
    </ToastContext.Provider>
  );
}
