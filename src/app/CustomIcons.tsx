import { useEffect, useState, type ReactNode } from 'react';
import { api } from '../api';
import { CustomIconsContext } from './contexts';

const NONE: ReadonlyMap<string, string> = new Map();

/** Loads the database's custom icons once it is unlocked; forgets them on lock. */
export function CustomIconsProvider({ unlocked, children }: { unlocked: boolean; children: ReactNode }) {
  const [icons, setIcons] = useState<{ unlocked: boolean; map: ReadonlyMap<string, string> }>({
    unlocked: false,
    map: NONE,
  });

  useEffect(() => {
    if (!unlocked) return;
    let live = true;
    api.getCustomIcons().then(
      (list) =>
        live &&
        setIcons({
          unlocked: true,
          map: new Map(list.map((i) => [i.id, `data:${i.mime};base64,${i.data}`])),
        }),
      // without them every item simply shows its standard icon
      () => {},
    );
    return () => {
      live = false;
    };
  }, [unlocked]);

  const value = unlocked && icons.unlocked ? icons.map : NONE;
  return <CustomIconsContext.Provider value={value}>{children}</CustomIconsContext.Provider>;
}
