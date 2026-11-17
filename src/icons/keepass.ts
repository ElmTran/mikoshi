// The 69 KeePass standard icons (IconID 0–68), drawn for Mikoshi in the same
// style as components/Icon.tsx: 24 × 24, round 1.6 px strokes in
// currentColor, no fills. Each icon is a list of SVG path `d` strings, so the
// same data feeds the React component and the static preview sheet
// (tools/icon-sheet.mjs). The meaning of each number follows KeePass
// (PwIcon); KeePass, KeePassXC and KeePassDX show their own pictures for it.

const f = (n: number) => String(Math.round(n * 100) / 100);

const circle = (cx: number, cy: number, r: number) =>
  `M${f(cx - r)} ${f(cy)}a${f(r)} ${f(r)} 0 1 0 ${f(2 * r)} 0a${f(r)} ${f(r)} 0 1 0 ${f(-2 * r)} 0`;

const rect = (x: number, y: number, w: number, h: number, r = 0) =>
  r
    ? `M${f(x + r)} ${f(y)}h${f(w - 2 * r)}a${f(r)} ${f(r)} 0 0 1 ${f(r)} ${f(r)}v${f(h - 2 * r)}` +
      `a${f(r)} ${f(r)} 0 0 1 ${f(-r)} ${f(r)}h${f(-(w - 2 * r))}a${f(r)} ${f(r)} 0 0 1 ${f(-r)} ${f(-r)}` +
      `v${f(-(h - 2 * r))}a${f(r)} ${f(r)} 0 0 1 ${f(r)} ${f(-r)}z`
    : `M${f(x)} ${f(y)}h${f(w)}v${f(h)}h${f(-w)}z`;

/** a round dot, from the round line cap */
const dot = (x: number, y: number) => `M${f(x)} ${f(y)}h.01`;

const polygon = (points: [number, number][]) => `M${points.map(([x, y]) => `${f(x)} ${f(y)}`).join('L')}z`;

const star = (cx: number, cy: number, outer: number, inner: number, n = 5) =>
  polygon(
    Array.from({ length: n * 2 }, (_, i): [number, number] => {
      const r = i % 2 ? inner : outer;
      const a = (Math.PI * i) / n - Math.PI / 2;
      return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
    }),
  );

const gear = (cx: number, cy: number, outer: number, inner: number, teeth: number) => {
  const step = (Math.PI * 2) / teeth;
  const at = (r: number, a: number): [number, number] => [cx + r * Math.cos(a), cy + r * Math.sin(a)];
  const points: [number, number][] = [];
  for (let k = 0; k < teeth; k++) {
    const a = k * step - Math.PI / 2;
    points.push(
      at(inner, a - step * 0.32),
      at(outer, a - step * 0.17),
      at(outer, a + step * 0.17),
      at(inner, a + step * 0.32),
    );
  }
  return polygon(points);
};

const folder = 'M3.5 7.5a2 2 0 0 1 2-2h3.7l2 2.2h7.3a2 2 0 0 1 2 2v7.8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z';

/** KeePass's names for the icons (PwIcon), by number. */
export const KEEPASS_ICON_NAMES = [
  'Key',
  'World',
  'Warning',
  'NetworkServer',
  'MarkedDirectory',
  'UserCommunication',
  'Parts',
  'Notepad',
  'WorldSocket',
  'Identity',
  'PaperReady',
  'Digicam',
  'IRCommunication',
  'MultiKeys',
  'Energy',
  'Scanner',
  'WorldStar',
  'CDRom',
  'Monitor',
  'EMail',
  'Configuration',
  'ClipboardReady',
  'PaperNew',
  'Screen',
  'EnergyCareful',
  'EMailBox',
  'Disk',
  'Drive',
  'PaperQ',
  'TerminalEncrypted',
  'Console',
  'Printer',
  'ProgramIcons',
  'Run',
  'Settings',
  'WorldComputer',
  'Archive',
  'Homebanking',
  'DriveWindows',
  'Clock',
  'EMailSearch',
  'PaperFlag',
  'Memory',
  'TrashBin',
  'Note',
  'Expired',
  'Info',
  'Package',
  'Folder',
  'FolderOpen',
  'FolderPackage',
  'LockOpen',
  'PaperLocked',
  'Checked',
  'Pen',
  'Thumbnail',
  'Book',
  'List',
  'UserKey',
  'Tool',
  'Home',
  'Star',
  'Tux',
  'Feather',
  'Apple',
  'Wiki',
  'Money',
  'Certificate',
  'BlackBerry',
] as const;

export const KEEPASS_ICONS: readonly (readonly string[])[] = [
  // 0 Key
  [circle(8, 15.5, 3.6), 'M10.6 13 19.5 4.1', 'M16.3 7.3l2.4 2.4', 'M13.9 9.7l1.8 1.8'],
  // 1 World
  [
    circle(12, 12, 8.5),
    'M3.5 12h17',
    'M12 3.5c2.4 2.4 3.6 5.2 3.6 8.5s-1.2 6.1-3.6 8.5c-2.4-2.4-3.6-5.2-3.6-8.5S9.6 5.9 12 3.5z',
  ],
  // 2 Warning
  ['M10.3 4.9a2 2 0 0 1 3.4 0l7.2 12.6a2 2 0 0 1-1.7 3H4.8a2 2 0 0 1-1.7-3z', 'M12 10v4', dot(12, 17)],
  // 3 NetworkServer
  [
    rect(4, 4.5, 16, 6, 1.6),
    rect(4, 13.5, 16, 6, 1.6),
    dot(7.5, 7.5),
    dot(7.5, 16.5),
    'M12 7.5h4.5',
    'M12 16.5h4.5',
  ],
  // 4 MarkedDirectory (a clipboard in the classic set)
  [rect(5.5, 5, 13, 15.5, 2), rect(9, 3.5, 6, 3, 1), 'M8.5 11h7', 'M8.5 14.5h7', 'M8.5 18h4'],
  // 5 UserCommunication
  [
    'M4.5 4.5h9A1.5 1.5 0 0 1 15 6v5.5a1.5 1.5 0 0 1-1.5 1.5H9l-3.5 3v-3h-1A1.5 1.5 0 0 1 3 11.5V6a1.5 1.5 0 0 1 1.5-1.5z',
    'M15 9h4.5a1.5 1.5 0 0 1 1.5 1.5V16a1.5 1.5 0 0 1-1.5 1.5h-1v3l-3.5-3h-4.5A1.5 1.5 0 0 1 9 16v-1.5',
  ],
  // 6 Parts
  ['M10.5 5a7.5 7.5 0 1 0 8 8h-8z', 'M13.5 3a7 7 0 0 1 7 7h-7z'],
  // 7 Notepad
  [
    'M13.5 4.5h-7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V12',
    'M8 9h3.5',
    'M8 16.5h6.5',
    'M19.6 3.9a1.6 1.6 0 0 1 0 2.3l-6.3 6.3-2.8.8.8-2.8 6.3-6.3a1.6 1.6 0 0 1 2 -.3z',
  ],
  // 8 WorldSocket
  ['M8 8h8v3.5a4 4 0 0 1-8 0z', 'M10 8V4.5', 'M14 8V4.5', 'M12 15.5V18a2.5 2.5 0 0 0 2.5 2.5H18'],
  // 9 Identity
  [
    rect(3, 5, 18, 14, 2),
    circle(8.5, 10.5, 2),
    'M5.5 16c.6-1.5 1.7-2.3 3-2.3s2.4.8 3 2.3',
    'M14 10h4',
    'M14 13.5h4',
  ],
  // 10 PaperReady (contacts)
  [
    rect(5, 3.5, 13.5, 17, 2),
    circle(11.75, 10, 2.4),
    'M7.9 16.5c.8-1.9 2.2-2.9 3.85-2.9s3.05 1 3.85 2.9',
    'M20.5 7.5v2.5',
    'M20.5 13v2.5',
  ],
  // 11 Digicam
  [
    'M4.5 8h3l1.5-2.5h6L16.5 8h3A1.5 1.5 0 0 1 21 9.5v8a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5v-8A1.5 1.5 0 0 1 4.5 8z',
    circle(12, 13, 3.3),
  ],
  // 12 IRCommunication
  [
    rect(8, 10, 8, 11, 2),
    circle(12, 13.6, 1.3),
    dot(10.5, 17.5),
    dot(13.5, 17.5),
    'M9.4 7a4 4 0 0 1 5.2 0',
    'M7.2 4.6a7.2 7.2 0 0 1 9.6 0',
  ],
  // 13 MultiKeys
  [
    circle(8, 7, 3.2),
    'M8 10.2v10.3',
    'M8 15.5h2.5',
    'M8 18.5h2',
    circle(16, 7, 3.2),
    'M16 10.2v10.3',
    'M16 15.5h2.5',
    'M16 18.5h2',
  ],
  // 14 Energy
  [rect(3, 7.5, 16, 9, 2), 'M21 10.5v3', 'M11.8 9 9.2 12.3h3.6L10.2 15.5'],
  // 15 Scanner
  [
    'M7 12.5V5.5a1 1 0 0 1 1-1h8a1 1 0 0 1 1 1v7',
    'M9.5 8h5',
    'M4.5 12.5h15A1.5 1.5 0 0 1 21 14v3.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5V14a1.5 1.5 0 0 1 1.5-1.5z',
    'M6 16h5',
    dot(17.5, 16),
  ],
  // 16 WorldStar
  [
    circle(10.5, 13.5, 7),
    'M3.5 13.5h14',
    'M10.5 6.5c1.9 1.9 2.8 4.2 2.8 7s-.9 5.1-2.8 7c-1.9-1.9-2.8-4.2-2.8-7s.9-5.1 2.8-7z',
    'M18.5 2.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z',
  ],
  // 17 CDRom
  [circle(12, 12, 8.5), circle(12, 12, 2.2), 'M6.8 10.5a5.5 5.5 0 0 1 3.7-3.7'],
  // 18 Monitor
  [rect(3.5, 4.5, 17, 11.5, 1.8), 'M9 19.5h6', 'M12 16v3.5'],
  // 19 EMail
  [rect(3, 5.5, 18, 13, 2), 'M3.5 7l8.5 6 8.5-6'],
  // 20 Configuration
  [gear(12, 12, 8.6, 6.4, 8), circle(12, 12, 2.6)],
  // 21 ClipboardReady (organizer)
  [
    rect(3.5, 5, 17, 15, 2),
    'M3.5 9.5h17',
    'M8 3v4',
    'M16 3v4',
    dot(8, 13),
    dot(12, 13),
    dot(16, 13),
    dot(8, 16.5),
    dot(12, 16.5),
  ],
  // 22 PaperNew
  [
    'M14 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5z',
    'M14 3.5v5h5',
    'M8.5 9h3',
    'M8.5 12.5h7',
    'M8.5 16h7',
  ],
  // 23 Screen
  [
    rect(4, 4, 6.5, 6.5, 1.5),
    rect(13.5, 4, 6.5, 6.5, 1.5),
    rect(4, 13.5, 6.5, 6.5, 1.5),
    rect(13.5, 13.5, 6.5, 6.5, 1.5),
  ],
  // 24 EnergyCareful (connection)
  [
    'M10.2 13.8a3.8 3.8 0 0 1 0-5.4l2.6-2.6a3.8 3.8 0 0 1 5.4 5.4l-1.3 1.3',
    'M13.8 10.2a3.8 3.8 0 0 1 0 5.4l-2.6 2.6a3.8 3.8 0 0 1-5.4-5.4l1.3-1.3',
  ],
  // 25 EMailBox
  [
    'M3.5 13.5 6 6.4A2 2 0 0 1 7.9 5h8.2A2 2 0 0 1 18 6.4l2.5 7.1v4a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z',
    'M3.5 13.5h5l1.2 2h4.6l1.2-2h5',
  ],
  // 26 Disk
  [
    'M5.5 3.5h10.8l4.2 4.2v10.8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2v-13a2 2 0 0 1 2-2z',
    'M8 3.5V8h7V3.5',
    'M7.5 20.5v-6h9v6',
  ],
  // 27 Drive (network drive)
  [
    rect(3.5, 4, 17, 8, 2),
    dot(16.5, 8),
    'M7 8h5',
    'M12 12v5',
    rect(10, 17, 4, 3.5, 1),
    'M4.5 18.75h5.5',
    'M14 18.75h5.5',
  ],
  // 28 PaperQ (media)
  [circle(12, 12, 8.5), 'M10 8.6v6.8l5.4-3.4z'],
  // 29 TerminalEncrypted
  [rect(3, 4, 18, 14.5, 2), 'M6.5 8.5 9 10.5l-2.5 2', rect(13, 11, 5, 4, 1), 'M14 11v-1a1.5 1.5 0 0 1 3 0v1'],
  // 30 Console
  [rect(3, 4.5, 18, 15, 2), 'M7 9.5l3 2.5-3 2.5', 'M12.5 15h4.5'],
  // 31 Printer
  [
    'M7 8.5v-4h10v4',
    'M7 17.5H5a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2',
    rect(7, 14, 10, 6.5, 0.6),
    dot(17, 11.5),
  ],
  // 32 ProgramIcons
  [
    rect(3.5, 4, 17, 16, 2),
    'M3.5 8h17',
    rect(6.5, 11, 4.5, 6, 0.8),
    'M13.5 11.5h4',
    'M13.5 14.25h4',
    'M13.5 17h2.5',
  ],
  // 33 Run
  [
    'M12 3c2.8 2.2 4 5.2 4 8.5V16H8v-4.5C8 8.2 9.2 5.2 12 3z',
    circle(12, 9.8, 1.6),
    'M8 12.5 5.5 15v3H8',
    'M16 12.5l2.5 2.5v3H16',
    'M10.5 18.5 12 21l1.5-2.5',
  ],
  // 34 Settings
  [
    'M4 7h9',
    'M17 7h3',
    circle(15, 7, 2),
    'M4 12h3',
    'M11 12h9',
    circle(9, 12, 2),
    'M4 17h11',
    'M19 17h1',
    circle(17, 17, 2),
  ],
  // 35 WorldComputer (remote desktop): the globe sits clear of the monitor
  [
    rect(2.5, 3, 11.5, 8.5, 1.5),
    'M8.25 11.5v3',
    'M6 14.5h4.5',
    circle(17.2, 16.8, 4.2),
    'M13 16.8h8.4',
    'M17.2 12.6c1.2 1.1 1.8 2.5 1.8 4.2s-.6 3.1-1.8 4.2c-1.2-1.1-1.8-2.5-1.8-4.2s.6-3.1 1.8-4.2z',
  ],
  // 36 Archive
  [rect(3, 4.5, 18, 4.5, 1.2), 'M4.5 9v9.5a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2V9', 'M10 13h4'],
  // 37 Homebanking
  [circle(12, 12, 8.5), 'M8.5 15.5l7-7', circle(9, 9, 1.4), circle(15, 15, 1.4)],
  // 38 DriveWindows (shared drive)
  [rect(3.5, 12, 17, 7, 2), dot(16.5, 15.5), 'M7 15.5h5', 'M12 9V3.5', 'M9.5 6 12 3.5 14.5 6'],
  // 39 Clock
  [circle(12, 12, 8.2), 'M12 7.8V12l2.8 1.8'],
  // 40 EMailSearch
  [rect(2.5, 4.5, 14, 10, 1.8), 'M3 6l6.5 4.6L16 6', circle(17.8, 17.5, 2.6), 'M19.7 19.4l1.8 1.8'],
  // 41 PaperFlag (vector graphics)
  [
    rect(3, 17, 4, 4, 1),
    rect(17, 3, 4, 4, 1),
    'M7 19c6.5 0 12-5.5 12-12',
    'M7 19h5',
    'M19 7v5',
    dot(13.5, 19),
    dot(19, 13.5),
  ],
  // 42 Memory
  [
    rect(3, 7, 18, 9, 1.5),
    rect(6, 9.5, 3, 4, 0.5),
    rect(10.5, 9.5, 3, 4, 0.5),
    rect(15, 9.5, 3, 4, 0.5),
    'M6 16v2.5M9 16v2.5M12 16v2.5M15 16v2.5M18 16v2.5',
  ],
  // 43 TrashBin
  ['M4.5 7h15', 'M9.5 7V4.8h5V7', 'M6.5 7l.9 12.2h9.2L17.5 7', 'M10.2 10.5v5.5', 'M13.8 10.5v5.5'],
  // 44 Note
  [
    'M5 4.5h14a1 1 0 0 1 1 1v9l-5 5H5a1 1 0 0 1-1-1v-13a1 1 0 0 1 1-1z',
    'M20 14.5h-4a1 1 0 0 0-1 1v4',
    'M7.5 9h9',
    'M7.5 12.5h5',
  ],
  // 45 Expired
  [circle(12, 12, 8.5), 'M9 9l6 6', 'M15 9l-6 6'],
  // 46 Info
  [circle(12, 12, 8.5), 'M12 11v5.5', dot(12, 7.8)],
  // 47 Package
  ['M12 3.5l8 4v9l-8 4-8-4v-9z', 'M4 7.5l8 4 8-4', 'M12 11.5v9', 'M8 5.5l8 4'],
  // 48 Folder
  [folder],
  // 49 FolderOpen
  [
    'M3.5 17.5v-11a1.5 1.5 0 0 1 1.5-1.5h3.7l2 2.2h7.3a1.5 1.5 0 0 1 1.5 1.5V10',
    'M3.5 18.5l2.7-7a1.5 1.5 0 0 1 1.4-1h13a1 1 0 0 1 .9 1.4l-2.4 6a1.5 1.5 0 0 1-1.4 1.1H3.5z',
  ],
  // 50 FolderPackage: a package inside a folder
  [folder, 'M12 10.4l3.4 1.7v3.6L12 17.4l-3.4-1.7v-3.6z', 'M8.6 12.1 12 13.8l3.4-1.7', 'M12 13.8v3.6'],
  // 51 LockOpen
  [rect(5, 10.5, 14, 9.5, 2.2), 'M8.2 10.5V8a3.8 3.8 0 0 1 7.3-1.5', 'M12 14.3v2'],
  // 52 PaperLocked
  [
    'M13 3.5H6.5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2H11',
    'M13 3.5V8h4.5',
    'M13 3.5l4.5 4.5v2.5',
    rect(13, 14, 8, 6.5, 1.3),
    'M14.8 14v-1.5a2.2 2.2 0 0 1 4.4 0V14',
  ],
  // 53 Checked
  [circle(12, 12, 8.5), 'M8 12.3l2.8 2.8 5.4-5.6'],
  // 54 Pen
  ['M12 3l5.5 7-3 7.5h-5l-3-7.5z', 'M12 3v8', circle(12, 12.4, 1.3), 'M9 20.5h6'],
  // 55 Thumbnail
  [rect(3.5, 4.5, 17, 15, 2), circle(9, 9.5, 1.8), 'M4 17.5l4.5-4.5 4 4 3-3 4.5 4.5'],
  // 56 Book
  [
    'M12 6.5c-1.8-1.3-4.3-2-7.5-2v13c3.2 0 5.7.7 7.5 2 1.8-1.3 4.3-2 7.5-2v-13c-3.2 0-5.7.7-7.5 2z',
    'M12 6.5v13',
  ],
  // 57 List
  ['M9 7h11', 'M9 12h11', 'M9 17h11', dot(4.5, 7), dot(4.5, 12), dot(4.5, 17)],
  // 58 UserKey
  [
    circle(9, 8, 3.3),
    'M3 19.5c.6-3.3 3-5.3 6-5.3 1.3 0 2.5.4 3.4 1',
    circle(17, 13.5, 2.3),
    'M17 15.8V21',
    'M17 18.3h1.8',
    'M17 20.2h1.3',
  ],
  // 59 Tool
  ['M12.5 4 20 11.5 17.5 14 10 6.5z', 'M15 11.5 6 20.5a1.5 1.5 0 0 1-2.1-2.1l9-9'],
  // 60 Home
  ['M4 10.5 12 4l8 6.5', 'M6 9v10a1 1 0 0 0 1 1h3.5v-5.5h3V20H17a1 1 0 0 0 1-1V9'],
  // 61 Star
  [star(12, 12.6, 8.6, 3.7)],
  // 62 Tux: a penguin (body, belly from under the beak, flippers, feet)
  [
    'M12 3c-2.4 0-3.8 1.9-3.8 4.4 0 1.3-.4 2.2-1.2 3.3C5.9 12.2 5 14 5 16c0 2.9 3 4.5 7 4.5s7-1.6 7-4.5c0-2-.9-3.8-2-5.3-.8-1.1-1.2-2-1.2-3.3C15.8 4.9 14.4 3 12 3z',
    'M12 9.8c-2 0-3.6 2.5-3.6 5.4 0 2.4 1.6 3.8 3.6 3.8s3.6-1.4 3.6-3.8c0-2.9-1.6-5.4-3.6-5.4z',
    dot(10.7, 6.6),
    dot(13.3, 6.6),
    'M10.9 8.2h2.2L12 9.4z',
    'M5.9 12.6 3.8 15.4',
    'M18.1 12.6l2.1 2.8',
    'M6.5 21.2h3.5',
    'M14 21.2h3.5',
  ],
  // 63 Feather
  [
    'M19.5 4.5c-6.5.5-11.5 5-12.5 12.5 7.5-1 12-6 12.5-12.5z',
    'M4.5 19.5 15 9',
    'M11.5 12.5h4.3',
    'M9.2 14.8h3.8',
  ],
  // 64 Apple (the fruit)
  [
    'M12 7.5c-1.5-1-3-1.3-4.5-1-2.6.6-4 3.2-3.5 6.5.5 3.5 2.5 6.5 4.5 7.3 1.2.5 2.3 0 3.5-.6 1.2.6 2.3 1.1 3.5.6 2-.8 4-3.8 4.5-7.3.5-3.3-.9-5.9-3.5-6.5-1.5-.3-3 0-4.5 1z',
    'M12 7.5c0-2 .8-3.3 2.5-4',
  ],
  // 65 Wiki
  [circle(12, 12, 8.5), 'M7 8.5l2 7 3-6 3 6 2-7'],
  // 66 Money
  [rect(2.5, 6.5, 19, 11, 1.8), circle(12, 12, 2.6), dot(6, 12), dot(18, 12)],
  // 67 Certificate
  [
    'M14.5 16.5h-9a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2h13a2 2 0 0 1 2 2v4',
    'M7 8.5h10',
    'M7 11.5h5',
    circle(17.5, 14.5, 2.5),
    'M16 16.6l-.8 4 2.3-1.2 2.3 1.2-.8-4',
  ],
  // 68 BlackBerry (phone with a keyboard)
  [
    rect(6, 2.5, 12, 19, 2.2),
    rect(8.5, 5, 7, 5, 0.8),
    dot(9.5, 13),
    dot(12, 13),
    dot(14.5, 13),
    dot(9.5, 15.8),
    dot(12, 15.8),
    dot(14.5, 15.8),
    'M10.5 18.8h3',
  ],
];

export const KEEPASS_ICON_COUNT = KEEPASS_ICONS.length;
