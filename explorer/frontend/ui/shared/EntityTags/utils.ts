import type { EntityTag } from './types';

import { route } from 'nextjs/routes';

import type { TMultichainContext } from 'lib/contexts/multichain';

export function getTagLinkParams(data: EntityTag, multichainContext?: TMultichainContext | null): { type: 'external' | 'internal'; href: string } | undefined {
  if (data.meta?.warpcastHandle) {
    return {
      type: 'external',
      href: `https://warpcast.com/${ data.meta.warpcastHandle }`,
    };
  }

  if (data.meta?.tagUrl) {
    return {
      type: 'external',
      href: data.meta.tagUrl,
    };
  }

  if (data.tagType === 'generic' || data.tagType === 'protocol') {
    return {
      type: 'internal',
      href: route({ pathname: '/accounts/label/[slug]', query: { slug: data.slug, tagType: data.tagType, tagName: data.name } }, multichainContext),
    };
  }
}

export function getTagName(data: EntityTag, addressHash?: string) {
  if (data.tagType === 'name' && data.meta?.cexDeposit === 'true' && addressHash) {
    return `${ data.name } (${ addressHash.slice(0, 2 + 5) })`;
  }

  return data.name;
}
