// Development only (`pnpm dev`, then open /?window=icons): every KeePass
// standard icon at the sizes the app uses, on plain ground and on the tiles.
// The same sheet as a standalone file: `node tools/icon-sheet.mjs`.

import { KEEPASS_ICONS, KEEPASS_ICON_NAMES } from '../icons/keepass';
import { KpIcon } from '../components/KpIcon';

export default function IconSheet() {
  return (
    <main className="icon-sheet">
      <h1>KeePass standard icons ({KEEPASS_ICONS.length})</h1>
      <div className="icon-grid">
        {KEEPASS_ICONS.map((_, id) => (
          <figure key={id}>
            <div className="icon-sizes">
              <KpIcon id={id} size={16} />
              <KpIcon id={id} size={24} />
              <KpIcon id={id} size={48} />
              <span className={`avatar md hue-${id % 6}`}>
                <KpIcon id={id} size={18} />
              </span>
            </div>
            <figcaption>
              {id} {KEEPASS_ICON_NAMES[id]}
            </figcaption>
          </figure>
        ))}
      </div>
    </main>
  );
}
