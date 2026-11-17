import { useCallback, useEffect, useState } from 'react';
import { api, listen } from '../api';
import type { VaultStatus } from '../api/types';
import { appWindow } from '../api/window';
import { TitleBar } from '../components/TitleBar';
import { UnlockScreen } from '../components/Unlock';
import { usePrefs } from './contexts';
import { CustomIconsProvider } from './CustomIcons';
import { Workspace } from './Workspace';

export function MainApp() {
  const { t } = usePrefs();
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
    const off = listen<VaultStatus>('vault-changed', setStatus);
    return () => void off.then((f) => f());
  }, [load]);

  // The window starts hidden so it never flashes unstyled; show it as soon as
  // there is something to show. Not via requestAnimationFrame: a hidden
  // window gets no animation frames, so the callback might never run.
  const ready = status !== null || failed;
  useEffect(() => {
    if (ready) void appWindow.show();
  }, [ready]);

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
