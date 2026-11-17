import { StrictMode } from 'react';
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

const quick = windowLabel() === 'quick';
// first guess before settings arrive, to avoid a flash of the wrong theme
document.documentElement.dataset.theme = systemTheme();
document.documentElement.classList.add(quick ? 'window-quick' : 'window-main');
if (!isTauri) document.documentElement.classList.add('browser');
if (import.meta.env.PROD) installAppBehaviour();

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <PrefsProvider>
        <ToastProvider>{quick ? <QuickApp /> : <MainApp />}</ToastProvider>
      </PrefsProvider>
    </StrictMode>,
  );
}
