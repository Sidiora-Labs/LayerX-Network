import { bech32 } from '@scure/base';
import React from 'react';

import type { SearchResult, SearchResultAddressOrContract, SearchResultItem } from 'types/api/search';

import type { Route } from 'nextjs-routes';

import useApiFetch from 'lib/api/useApiFetch';

export const PAX_BECH32_PREFIX = 'pax';
export const PAX_BECH32_SEPARATOR = '1';
export const LAYERX_DID_PREFIX = 'did:layerx:';

const EVM_ADDRESS_REGEXP = /^0x[\da-f]{40}$/i;
const TX_HASH_REGEXP = /^0x[\da-f]{64}$/i;
const HEX_64_REGEXP = /^[\da-f]{64}$/i;
const PAX_ADDRESS_BYTE_LENGTH = 20;

export type PaxeerXIdentifierType = 'pax_address' | 'layerx_did' | 'kernel_account';

export interface PaxeerXIdentifier {
  type: PaxeerXIdentifierType;
  hash: string;
}

export function isEvmAddressTerm(term: string): boolean {
  return EVM_ADDRESS_REGEXP.test(term.trim());
}

export function isTxHashTerm(term: string): boolean {
  return TX_HASH_REGEXP.test(term.trim());
}

export function isPaxBech32Address(term: string): boolean {
  const value = term.trim();

  if (!value.startsWith(`${ PAX_BECH32_PREFIX }${ PAX_BECH32_SEPARATOR }`)) {
    return false;
  }

  try {
    const { prefix, words } = bech32.decode(value as `${ string }${ typeof PAX_BECH32_SEPARATOR }${ string }`);

    if (prefix !== PAX_BECH32_PREFIX) {
      return false;
    }

    return bech32.fromWords(words).length === PAX_ADDRESS_BYTE_LENGTH;
  } catch (error) {
    return false;
  }
}

export function isLayerXDid(term: string): boolean {
  const value = term.trim();

  if (!value.startsWith(LAYERX_DID_PREFIX)) {
    return false;
  }

  return HEX_64_REGEXP.test(value.slice(LAYERX_DID_PREFIX.length));
}

export function isKernelAccountId(term: string): boolean {
  return HEX_64_REGEXP.test(term.trim());
}

export function parsePaxeerXIdentifier(term: string): PaxeerXIdentifier | undefined {
  const value = term.trim();

  if (!value || isEvmAddressTerm(value) || isTxHashTerm(value)) {
    return undefined;
  }

  if (isPaxBech32Address(value)) {
    return { type: 'pax_address', hash: value };
  }

  if (isLayerXDid(value)) {
    return { type: 'layerx_did', hash: value };
  }

  if (isKernelAccountId(value)) {
    return { type: 'kernel_account', hash: value };
  }

  return undefined;
}

export function findSearchResultAddressHash(items: Array<SearchResultItem>): string | undefined {
  const addressItem = items.find((item): item is SearchResultAddressOrContract => item.type === 'address' || item.type === 'contract');

  return addressItem?.address_hash;
}

export function getSearchRedirectRoute(term: string, redirect: boolean, resolvedAddressHash?: string): Route {
  if (resolvedAddressHash && parsePaxeerXIdentifier(term)) {
    return { pathname: '/paxeer-x/account/[hash]', query: { hash: resolvedAddressHash } };
  }

  return { pathname: '/search-results', query: { q: term, redirect: redirect ? 'true' : 'false' } };
}

export function useSearchRedirect() {
  const apiFetch = useApiFetch();

  return React.useCallback(async(term: string, redirect: boolean): Promise<Route> => {
    const identifier = parsePaxeerXIdentifier(term);

    if (!identifier) {
      return getSearchRedirectRoute(term, redirect);
    }

    const result = await apiFetch<'general:search', SearchResult>('general:search', {
      queryParams: { q: identifier.hash },
      logError: true,
    }).catch(() => undefined);

    const addressHash = result && 'items' in result ? findSearchResultAddressHash(result.items) : undefined;

    return getSearchRedirectRoute(term, redirect, addressHash);
  }, [ apiFetch ]);
}
