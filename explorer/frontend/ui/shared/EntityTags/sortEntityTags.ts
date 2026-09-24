import type { EntityTag } from './types';

export default function sortEntityTags(tagA: EntityTag, tagB: EntityTag): number {
  if (tagA.ordinal < tagB.ordinal) {
    return 1;
  }

  if (tagA.ordinal > tagB.ordinal) {
    return -1;
  }

  return 0;
}
