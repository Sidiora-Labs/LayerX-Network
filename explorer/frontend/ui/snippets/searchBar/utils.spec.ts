import { describe, it, expect } from 'vitest';

import {
  getSearchRedirectRoute,
  isKernelAccountId,
  isLayerXDid,
  isPaxBech32Address,
  parsePaxeerXIdentifier,
} from './utils';

const PAX_ADDRESS = 'pax1zg69v7ys40x77y352eufp27daufrg4nc0aa6dg';
const PAX_ADDRESS_ALT = 'pax167y6vp7w4shsu9yx0hjwkzen557uldaqfj7dct';
const KERNEL_ACCOUNT = 'b5c3f1a0d29e4867b1f0a4c73d5e9182ac6b70d34f81e2905c7ab6134de8f072';
const LAYERX_DID = `did:layerx:${ KERNEL_ACCOUNT }`;
const EVM_ADDRESS = '0x1234567890abcdef1234567890abcdef12345678';
const TX_HASH = `0x${ KERNEL_ACCOUNT }`;

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

describe('getSearchRedirectRoute', () => {
  it('routes every Paxeer X identifier to the unified account page', () => {
    expect(getSearchRedirectRoute(PAX_ADDRESS)).toEqual({ pathname: '/paxeer-x/account/[hash]', query: { hash: PAX_ADDRESS } });
    expect(getSearchRedirectRoute(LAYERX_DID)).toEqual({ pathname: '/paxeer-x/account/[hash]', query: { hash: LAYERX_DID } });
    expect(getSearchRedirectRoute(KERNEL_ACCOUNT)).toEqual({ pathname: '/paxeer-x/account/[hash]', query: { hash: KERNEL_ACCOUNT } });
  });

  it('does not redirect EVM addresses or transaction hashes', () => {
    expect(getSearchRedirectRoute(EVM_ADDRESS)).toBeUndefined();
    expect(getSearchRedirectRoute(TX_HASH)).toBeUndefined();
  });
});
