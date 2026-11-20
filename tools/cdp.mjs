// Drives a running Mikoshi window over the Chrome DevTools Protocol, so the
// real app (real Rust backend, real CSP) can be checked without touching the
// mouse, keyboard or focus of the desktop.
//
// Development only. Start the app with remote debugging turned on:
//
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333'
//   pnpm tauri dev
//
// Usage (window is a Tauri window label: main | quick):
//   node tools/cdp.mjs list
//   node tools/cdp.mjs <window> eval "<js expression>"     awaits promises, prints JSON
//   node tools/cdp.mjs <window> shot <file.png> [rrggbb]   colour standing in for the Mica backdrop
//   node tools/cdp.mjs <window> key <Key> [ctrl] [shift] [alt]
//   node tools/cdp.mjs <window> type "<text>"
//   node tools/cdp.mjs <window> click "<css selector>"
//   node tools/cdp.mjs <window> hover "<css selector>"
//   node tools/cdp.mjs <window> logs [ms]                  reload, print console output while starting
//
// <window> may also be a number (position in `list`), for when a page failed
// to start and has no label.

import { writeFileSync } from 'node:fs';

const PORT = process.env.CDP_PORT ?? '9333';
const BASE = `http://127.0.0.1:${PORT}`;

class Session {
  #ws;
  #next = 1;
  #pending = new Map();
  #listeners = new Map();

  static async open(url) {
    const s = new Session();
    s.#ws = new WebSocket(url);
    await new Promise((resolve, reject) => {
      s.#ws.addEventListener('open', resolve, { once: true });
      s.#ws.addEventListener('error', () => reject(new Error(`cannot connect to ${url}`)), { once: true });
    });
    s.#ws.addEventListener('message', (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.method) {
        s.#listeners.get(msg.method)?.(msg.params);
        return;
      }
      const p = s.#pending.get(msg.id);
      if (!p) return;
      s.#pending.delete(msg.id);
      if (msg.error) p.reject(new Error(msg.error.message));
      else p.resolve(msg.result);
    });
    return s;
  }

  on(event, handler) {
    this.#listeners.set(event, handler);
  }

  send(method, params = {}) {
    const id = this.#next++;
    return new Promise((resolve, reject) => {
      this.#pending.set(id, { resolve, reject });
      this.#ws.send(JSON.stringify({ id, method, params }));
    });
  }

  async eval(expression) {
    const r = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) {
      throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    }
    return r.result.value;
  }

  close() {
    this.#ws.close();
  }
}

async function pages() {
  const res = await fetch(`${BASE}/json/list`).catch(() => null);
  if (!res?.ok) throw new Error(`no debugger on port ${PORT}; is the app running with remote debugging?`);
  return (await res.json()).filter((t) => t.type === 'page');
}

const LABEL = 'window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? null';

async function describe() {
  const out = [];
  for (const t of await pages()) {
    const s = await Session.open(t.webSocketDebuggerUrl);
    out.push({
      label: await s.eval(LABEL).catch(() => null),
      url: t.url,
      title: t.title,
      ws: t.webSocketDebuggerUrl,
    });
    s.close();
  }
  return out;
}

async function connect(label) {
  const all = await describe();
  const target = /^\d+$/.test(label) ? all[Number(label)] : all.find((t) => t.label === label);
  if (!target) throw new Error(`no window "${label}"`);
  return Session.open(target.ws);
}

const KEYS = {
  Enter: { code: 'Enter', vk: 13, text: '\r' },
  Escape: { code: 'Escape', vk: 27 },
  Delete: { code: 'Delete', vk: 46 },
  Tab: { code: 'Tab', vk: 9 },
  ArrowUp: { code: 'ArrowUp', vk: 38 },
  ArrowDown: { code: 'ArrowDown', vk: 40 },
  ',': { code: 'Comma', vk: 188 },
};

function keyInfo(key) {
  if (KEYS[key]) return { key, ...KEYS[key] };
  if (/^[a-z]$/i.test(key)) {
    return { key: key.toLowerCase(), code: `Key${key.toUpperCase()}`, vk: key.toUpperCase().charCodeAt(0) };
  }
  throw new Error(`unknown key "${key}"`);
}

async function center(s, selector) {
  const box = await s.eval(`(() => {
    const el = document.querySelector(${JSON.stringify(selector)});
    if (!el) return null;
    el.scrollIntoView({ block: 'nearest' });
    const r = el.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`);
  if (!box) throw new Error(`nothing matches ${selector}`);
  return box;
}

async function main() {
  const [first, cmd, ...rest] = process.argv.slice(2);
  if (first === 'list') {
    console.log(
      JSON.stringify(
        (await describe()).map(({ ws, ...t }) => t),
        null,
        2,
      ),
    );
    return;
  }
  if (!first || !cmd)
    throw new Error('usage: node tools/cdp.mjs <window> <eval|shot|key|type|click|hover> ...');
  const s = await connect(first);
  try {
    switch (cmd) {
      case 'eval':
        console.log(JSON.stringify(await s.eval(rest[0]), null, 2));
        break;
      case 'shot': {
        // A transparent page is captured on white. To stand in for the Mica
        // backdrop, paint the page background for the duration of the shot.
        const [file, bg] = rest;
        const paint = (value) =>
          s.eval(
            `document.documentElement.style.setProperty('background', ${JSON.stringify(value)}, 'important')`,
          );
        if (bg) await paint(`#${bg}`);
        const { data } = await s.send('Page.captureScreenshot', { format: 'png' });
        if (bg) await s.eval(`document.documentElement.style.removeProperty('background')`);
        writeFileSync(file, Buffer.from(data, 'base64'));
        console.log(`saved ${file}`);
        break;
      }
      case 'key': {
        const [key, ...mods] = rest;
        const k = keyInfo(key);
        const modifiers =
          (mods.includes('alt') ? 1 : 0) | (mods.includes('ctrl') ? 2 : 0) | (mods.includes('shift') ? 8 : 0);
        const base = { key: k.key, code: k.code, windowsVirtualKeyCode: k.vk, modifiers };
        // with Ctrl or Alt held, a key produces no text
        const text = modifiers & 3 ? undefined : k.text;
        await s.send('Input.dispatchKeyEvent', { type: text ? 'keyDown' : 'rawKeyDown', ...base, text });
        await s.send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
        break;
      }
      case 'type':
        await s.send('Input.insertText', { text: rest[0] });
        break;
      case 'hover':
      case 'click': {
        const { x, y } = await center(s, rest[0]);
        await s.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
        if (cmd === 'click') {
          const press = { x, y, button: 'left', clickCount: 1 };
          await s.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...press });
          await s.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...press });
        }
        break;
      }
      case 'logs': {
        // Reloads the page and prints what it writes to the console while
        // starting up (errors, warnings, CSP violations), for `ms` milliseconds.
        const ms = Number(rest[0] ?? 2500);
        const lines = [];
        s.on('Runtime.consoleAPICalled', (p) =>
          lines.push(`[${p.type}] ${p.args.map((a) => a.value ?? a.description ?? '').join(' ')}`),
        );
        s.on('Runtime.exceptionThrown', (p) =>
          lines.push(`[exception] ${p.exceptionDetails.exception?.description ?? p.exceptionDetails.text}`),
        );
        s.on('Log.entryAdded', (p) => lines.push(`[${p.entry.source}/${p.entry.level}] ${p.entry.text}`));
        await s.send('Runtime.enable');
        await s.send('Log.enable');
        await s.send('Page.reload');
        await new Promise((r) => setTimeout(r, ms));
        console.log(lines.length ? lines.join('\n') : '(nothing logged)');
        break;
      }
      default:
        throw new Error(`unknown command "${cmd}"`);
    }
  } finally {
    s.close();
  }
}

main().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
