// Window controls. No-ops outside Tauri so the UI also runs in a browser.

import { LogicalSize } from '@tauri-apps/api/dpi';
import { getCurrentWindow, type Theme } from '@tauri-apps/api/window';
import { isTauri } from './index';

const win = () => getCurrentWindow();

export const windowLabel = (): string =>
  isTauri ? win().label : (new URLSearchParams(location.search).get('window') ?? 'main');

export const appWindow = {
  show: async () => {
    if (isTauri) await win().show();
  },
  focus: async () => {
    if (isTauri) await win().setFocus();
  },
  minimize: async () => {
    if (isTauri) await win().minimize();
  },
  toggleMaximize: async () => {
    if (isTauri) await win().toggleMaximize();
  },
  close: async () => {
    if (isTauri) await win().close();
  },
  isMaximized: async () => (isTauri ? win().isMaximized() : false),
  setTheme: async (theme: Theme | null) => {
    if (isTauri) await win().setTheme(theme);
  },
  /** the window's current theme; null outside Tauri */
  theme: async (): Promise<Theme | null> => (isTauri ? win().theme() : null),
  /** in logical pixels */
  setSize: async (width: number, height: number) => {
    if (isTauri) await win().setSize(new LogicalSize(width, height));
  },
  onResized: async (fn: () => void): Promise<() => void> => (isTauri ? win().onResized(fn) : () => {}),
};
