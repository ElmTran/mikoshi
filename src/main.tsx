import { lazy, StrictMode, Suspense } from 'react';
import { createRoot } from 'react-dom/client';
import { isTauri } from './api';
import { windowLabel } from './api/window';
import { installAppBehaviour } from './app/appShell';
import { MainApp } from './app/MainApp';
import { PrefsProvider } from './app/PrefsProvider';
import { QuickApp } from './app/QuickApp';
import { ToastProvider } from './app/ToastProvider';
import { systemTheme } from './theme';
import './styles/tokens.css';
import './styles/base.css';
import './styles/main.css';
import './styles/quick.css';

const label = windowLabel();
const quick = label === 'quick';
// first guess before settings arrive, to avoid a flash of the wrong theme
document.documentElement.dataset.theme = systemTheme();
document.documentElement.classList.add(quick ? 'window-quick' : 'window-main');
if (!isTauri) document.documentElement.classList.add('browser');
if (import.meta.env.PROD) installAppBehaviour();

// development only: the icon preview sheet (/?window=icons); not in production builds
const IconSheet = import.meta.env.DEV && label === 'icons' ? lazy(() => import('./app/IconSheet')) : null;

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      {IconSheet ? (
        <Suspense>
          <IconSheet />
        </Suspense>
      ) : (
        <PrefsProvider>
          <ToastProvider>{quick ? <QuickApp /> : <MainApp />}</ToastProvider>
        </PrefsProvider>
      )}
    </StrictMode>,
  );
}
