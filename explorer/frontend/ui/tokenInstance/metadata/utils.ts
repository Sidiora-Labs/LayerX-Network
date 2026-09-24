import { upperFirst } from 'es-toolkit';

export function formatName(_name: string) {
  const name = _name
    .replaceAll('_', ' ')
    .replaceAll(/\burl|nft|id\b/gi, (str) => str.toUpperCase());

  return upperFirst(name.trim());
}

const PINNED_FIELDS = [ 'name', 'description' ];

export function sortFields([ nameA ]: [string, unknown], [ nameB ]: [string, unknown]): number {
  const pinnedIndexA = PINNED_FIELDS.indexOf(nameA.toLowerCase());
  const pinnedIndexB = PINNED_FIELDS.indexOf(nameB.toLowerCase());

  if (pinnedIndexA === -1 && pinnedIndexB === -1) {
    return 0;
  }

  if (pinnedIndexB === -1) {
    return -1;
  }

  if (pinnedIndexA === -1) {
    return 1;
  }

  return pinnedIndexA > pinnedIndexB ? 1 : -1;
}
