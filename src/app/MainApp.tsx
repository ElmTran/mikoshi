import { useCallback, useEffect, useRef, useState } from 'react';
import { api, listen } from '../api';
import type { ErrorCode, VaultStatus } from '../api/types';
import { appWindow } from '../api/window';
import { TitleBar } from '../components/TitleBar';
import { UnlockScreen } from '../components/Unlock';
import { usePrefs, useToast } from './contexts';
import { CustomIconsProvider } from './CustomIcons';
import { Workspace } from './Workspace';

export function MainApp() {
  const { t, info, settings } = usePrefs();
  const toast = useToast();
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [failed, setFailed] = useState(false);

  const load = useCallback(() => {
    api.vaultStatus().then(
      (s) => {
        setStatus(s);
        setFailed(false);
      },
      () => setFailed(true),
    );
  }, []);

  useEffect(() => {
    load();
    const offs = [
      listen<VaultStatus>('vault-changed', setStatus),
      // edits change the unsaved mark in the title bar
      listen<VaultStatus>('vault-edited', setStatus),
    ];
    return () => offs.forEach((p) => void p.then((f) => f()));
  }, [load]);

  // auto-type that failed after Mikoshi got out of the way
  useEffect(() => {
    const off = listen<{ code: ErrorCode }>('autotype-failed', ({ code }) =>
      toast({ message: t(`error.${code}`), tone: 'error', seconds: 8 }),
    );
    return () => void off.then((f) => f());
  }, [t, toast]);

  // The window starts hidden so it never flashes unstyled; show it as soon as
  // there is something to show. Not via requestAnimationFrame: a hidden
  // window gets no animation frames, so the callback might never run.
  // Started with Windows, it stays in the tray until asked for.
  const ready = (status !== null && info !== null && !info.startHidden) || failed;
  useEffect(() => {
    if (ready) void appWindow.show();
  }, [ready]);

  // a global hotkey that did not register at startup (features.md §3.6)
  const shown = ready && status !== null;
  const checked = useRef(false);
  useEffect(() => {
    if (!shown || checked.current) return;
    checked.current = true;
    api.hotkeyStatus().then(
      (h) => {
        const bad = [
          [h.quickSearch, settings.quickSearchHotkey],
          [h.lock, settings.lockHotkey],
        ].filter(([state]) => state === 'taken' || state === 'invalid');
        if (bad.length)
          toast({
            message: t('toast.hotkeysUnavailable', { keys: bad.map(([, k]) => k).join(', ') }),
            tone: 'error',
            seconds: 10,
          });
      },
      () => {},
    );
  }, [shown, settings, t, toast]);

  if (!status) {
    // Never leave a blank window: say what happened and offer a way out.
    return failed ? (
      <div className="shell locked">
        <TitleBar />
        <main className="startup-error" role="alert">
          <h1>{t('startup.failed')}</h1>
          <p>{t('startup.failedHint')}</p>
          <button className="btn primary" onClick={load}>
            {t('startup.retry')}
          </button>
        </main>
      </div>
    ) : null;
  }
  return status.locked ? (
    <div className="shell locked" key="locked">
      <TitleBar />
      <UnlockScreen status={status} />
    </div>
  ) : (
    <CustomIconsProvider unlocked key="open">
      <Workspace status={status} />
    </CustomIconsProvider>
  );
}
