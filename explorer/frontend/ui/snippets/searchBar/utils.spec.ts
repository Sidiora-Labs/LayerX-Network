import * as searchMock from 'mocks/search/index';
import { describe, it, expect } from 'vitest';

import {
  findSearchResultAddressHash,
  getSearchRedirectRoute,
  isKernelAccountId,
  isLayerXDid,
  isPaxBech32Address,
  parsePaxeerXIdentifier,
} from './utils';

const PAX_ADDRESS = 'pax1zg69v7ys40x77y352eufp27daufrg4nc0aa6dg';
const PAX_ADDRESS_ALT = 'pax167y6vp7w4shsu9yx0hjwkzen557uldaqfj7dct';
const KERNEL_ACCOUNT = 'b5c3f1a0d29e4867b1f0a4c73d5e9182ac6b70d34f81e2905c7ab6134de8f072';
const KERNEL_KEY = '9f2c1b4d5e6a7b8c9d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e';
const LAYERX_DID = `did:layerx:${ KERNEL_ACCOUNT }`;
const EVM_ADDRESS = '0x1234567890abcdef1234567890abcdef12345678';
const TX_HASH = `0x${ KERNEL_ACCOUNT }`;
const RESOLVED_ADDRESS = '0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131a';

describe('isPaxBech32Address', () => {
  it('accepts a bech32 address with the pax prefix', () => {
    expect(isPaxBech32Address(PAX_ADDRESS)).toBe(true);
    expect(isPaxBech32Address(PAX_ADDRESS_ALT)).toBe(true);
  });

  it('rejects a bech32 address with another prefix', () => {
    expect(isPaxBech32Address('cosmos1zg69v7ys40x77y352eufp27daufrg4ncnjqz7q')).toBe(false);
  });

  it('rejects a pax string with a broken checksum', () => {
    expect(isPaxBech32Address('pax1zg69v7ys40x77y352eufp27daufrg4nc0aa6dh')).toBe(false);
  });
});

describe('isLayerXDid', () => {
  it('accepts a LayerX DID', () => {
    expect(isLayerXDid(LAYERX_DID)).toBe(true);
  });

  it('rejects a DID with a payload that is not 32 bytes', () => {
    expect(isLayerXDid('did:layerx:b5c3f1a0')).toBe(false);
  });
});

describe('isKernelAccountId', () => {
  it('accepts a bare 32-byte hex account id', () => {
    expect(isKernelAccountId(KERNEL_ACCOUNT)).toBe(true);
  });

  it('accepts a bare 32-byte kernel key', () => {
    expect(isKernelAccountId(KERNEL_KEY)).toBe(true);
  });

  it('rejects a shorter hex string', () => {
    expect(isKernelAccountId(KERNEL_ACCOUNT.slice(0, 40))).toBe(false);
  });
});

describe('parsePaxeerXIdentifier', () => {
  it('classifies a pax bech32 address', () => {
    expect(parsePaxeerXIdentifier(PAX_ADDRESS)).toEqual({ type: 'pax_address', hash: PAX_ADDRESS });
  });

  it('classifies a LayerX DID', () => {
    expect(parsePaxeerXIdentifier(LAYERX_DID)).toEqual({ type: 'layerx_did', hash: LAYERX_DID });
  });

  it('classifies a kernel account id', () => {
    expect(parsePaxeerXIdentifier(KERNEL_ACCOUNT)).toEqual({ type: 'kernel_account', hash: KERNEL_ACCOUNT });
  });

  it('classifies a bare kernel key', () => {
    expect(parsePaxeerXIdentifier(KERNEL_KEY)).toEqual({ type: 'kernel_account', hash: KERNEL_KEY });
  });

  it('classifies an upper case DID payload', () => {
    const did = `did:layerx:${ KERNEL_ACCOUNT.toUpperCase() }`;

    expect(parsePaxeerXIdentifier(did)).toEqual({ type: 'layerx_did', hash: did });
  });

  it('leaves the agent account id spelling alone', () => {
    expect(parsePaxeerXIdentifier(`agent:did:layerx:${ KERNEL_ACCOUNT }:main`)).toBeUndefined();
  });

  it('trims surrounding whitespace', () => {
    expect(parsePaxeerXIdentifier(`  ${ LAYERX_DID }  `)).toEqual({ type: 'layerx_did', hash: LAYERX_DID });
  });

  it('leaves EVM addresses alone', () => {
    expect(parsePaxeerXIdentifier(EVM_ADDRESS)).toBeUndefined();
  });

  it('leaves transaction hashes alone', () => {
    expect(parsePaxeerXIdentifier(TX_HASH)).toBeUndefined();
  });

  it('leaves free text and empty input alone', () => {
    expect(parsePaxeerXIdentifier('vitalik.eth')).toBeUndefined();
    expect(parsePaxeerXIdentifier('')).toBeUndefined();
    expect(parsePaxeerXIdentifier('   ')).toBeUndefined();
  });
});

describe('findSearchResultAddressHash', () => {
  it('returns the hash of the first address result', () => {
    expect(findSearchResultAddressHash([ searchMock.token1, searchMock.address1, searchMock.address2 ])).toBe(searchMock.address1.address_hash);
  });

  it('returns the hash of a contract result', () => {
    expect(findSearchResultAddressHash([ searchMock.token1, searchMock.contract1 ])).toBe(searchMock.contract1.address_hash);
  });

  it('returns nothing when no result carries an address', () => {
    expect(findSearchResultAddressHash([ searchMock.token1, searchMock.block1 ])).toBeUndefined();
    expect(findSearchResultAddressHash([])).toBeUndefined();
  });
});

describe('getSearchRedirectRoute', () => {
  it('routes a Paxeer X identifier to the unified account page of the address it resolved to', () => {
    expect(getSearchRedirectRoute(PAX_ADDRESS, true, RESOLVED_ADDRESS))
      .toEqual({ pathname: '/paxeer-x/account/[hash]', query: { hash: RESOLVED_ADDRESS } });
    expect(getSearchRedirectRoute(LAYERX_DID, true, RESOLVED_ADDRESS))
      .toEqual({ pathname: '/paxeer-x/account/[hash]', query: { hash: RESOLVED_ADDRESS } });
    expect(getSearchRedirectRoute(KERNEL_ACCOUNT, true, RESOLVED_ADDRESS))
      .toEqual({ pathname: '/paxeer-x/account/[hash]', query: { hash: RESOLVED_ADDRESS } });
    expect(getSearchRedirectRoute(KERNEL_KEY, true, RESOLVED_ADDRESS))
      .toEqual({ pathname: '/paxeer-x/account/[hash]', query: { hash: RESOLVED_ADDRESS } });
  });

  it('falls through to the search results when an identifier resolved to nothing', () => {
    expect(getSearchRedirectRoute(PAX_ADDRESS, true))
      .toEqual({ pathname: '/search-results', query: { q: PAX_ADDRESS, redirect: 'true' } });
    expect(getSearchRedirectRoute(LAYERX_DID, false))
      .toEqual({ pathname: '/search-results', query: { q: LAYERX_DID, redirect: 'false' } });
  });

  it('keeps the search results route for EVM addresses and transaction hashes', () => {
    expect(getSearchRedirectRoute(EVM_ADDRESS, true, RESOLVED_ADDRESS))
      .toEqual({ pathname: '/search-results', query: { q: EVM_ADDRESS, redirect: 'true' } });
    expect(getSearchRedirectRoute(TX_HASH, false, RESOLVED_ADDRESS))
      .toEqual({ pathname: '/search-results', query: { q: TX_HASH, redirect: 'false' } });
  });
});
