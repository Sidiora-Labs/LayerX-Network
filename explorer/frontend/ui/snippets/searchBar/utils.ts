import { bech32 } from '@scure/base';

import type { Route } from 'nextjs-routes';

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

export function getSearchRedirectRoute(term: string): Route | undefined {
  const identifier = parsePaxeerXIdentifier(term);

  if (!identifier) {
    return undefined;
  }

  return { pathname: '/paxeer-x/account/[hash]', query: { hash: identifier.hash } };
}
