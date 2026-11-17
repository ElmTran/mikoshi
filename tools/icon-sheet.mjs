// Writes design/icons/index.html: the 69 KeePass standard icons drawn for
// Mikoshi, at 16, 24 and 48 px, light and dark, plus on the coloured entry
// tiles. Open the file in a browser to review the set; no server needed.
//
//   node tools/icon-sheet.mjs
//
// Node 22.18+ loads the TypeScript source directly (type stripping).

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { KEEPASS_ICONS, KEEPASS_ICON_NAMES } from '../src/icons/keepass.ts';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'design', 'icons', 'index.html');

const svg = (paths, size) =>
  `<svg width="${size}" height="${size}" viewBox="0 0 24 24" aria-hidden="true">${paths
    .map((d) => `<path d="${d}"/>`)
    .join('')}</svg>`;

// the tile colours from src/styles/main.css (.avatar.hue-*)
const HUES = ['#0f6cbd', '#8764b8', '#ca5010', '#038387', '#c239b3', '#498205'];

const cards = KEEPASS_ICONS.map(
  (paths, id) => `
    <figure>
      <div class="sizes">${svg(paths, 16)}${svg(paths, 24)}${svg(paths, 48)}
        <span class="tile" style="background:${HUES[id % 6]}">${svg(paths, 18)}</span></div>
      <figcaption>${id} ${KEEPASS_ICON_NAMES[id]}</figcaption>
    </figure>`,
).join('');

const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Mikoshi: KeePass standard icons</title>
<style>
  body { margin: 0; font: 14px/20px 'Segoe UI', 'Microsoft YaHei UI', sans-serif; }
  section { padding: 24px 32px; }
  .light { background: #f3f3f3; color: rgba(0,0,0,.9); }
  .dark { background: #202020; color: #fff; }
  h1 { font-size: 20px; margin: 0 0 16px; font-weight: 600; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(210px, 1fr)); gap: 8px; }
  figure { margin: 0; padding: 12px; border-radius: 8px; border: 1px solid rgba(128,128,128,.18); }
  .light figure { background: rgba(255,255,255,.72); }
  .dark figure { background: rgba(255,255,255,.05); }
  .sizes { display: flex; align-items: center; gap: 14px; }
  svg { flex: none; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .tile { width: 32px; height: 32px; border-radius: 6px; display: inline-grid; place-items: center; color: #fff; }
  .tile svg { stroke-width: 1.7; }
  figcaption { margin-top: 8px; font-size: 12px; opacity: .7; }
</style>
</head>
<body>
<section class="light"><h1>KeePass standard icons (${KEEPASS_ICONS.length}) · light</h1><div class="grid">${cards}</div></section>
<section class="dark"><h1>KeePass standard icons (${KEEPASS_ICONS.length}) · dark</h1><div class="grid">${cards}</div></section>
</body>
</html>
`;

mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, html);
console.log(`wrote ${out}`);
