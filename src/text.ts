// Small text helpers shared by the windows.

/** The host part of a URL: `https://github.com/login` → `github.com`. */
export const host = (url: string) => url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '').split('/')[0] ?? url;

/** The last part of a Windows or POSIX path. */
export const baseName = (path: string) => path.split(/[/\\]/).pop() ?? path;
