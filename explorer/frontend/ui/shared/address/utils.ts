import type { AddressParam } from 'types/api/addressParams';

export type TxCourseType = 'in' | 'out' | 'self' | 'unspecified';

export function getTxCourseType(from: string, to: string | undefined, current?: string): TxCourseType {
  if (current === undefined) {
    return 'unspecified';
  }

  const fromLower = from.toLowerCase();
  const toLower = to?.toLowerCase();
  const currentLower = current.toLowerCase();

  if (toLower && fromLower === toLower && fromLower === currentLower) {
    return 'self';
  }

  if (fromLower === currentLower) {
    return 'out';
  }

  if (toLower && toLower === currentLower) {
    return 'in';
  }

  return 'unspecified';
}

export const unknownAddress: Omit<AddressParam, 'hash'> = {
  is_contract: false,
  is_verified: false,
  implementations: null,
  name: '',
  private_tags: [],
  public_tags: [],
  watchlist_names: [],
  ens_domain_name: null,
};
