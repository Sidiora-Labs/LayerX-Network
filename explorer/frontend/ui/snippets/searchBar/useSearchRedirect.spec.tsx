// @vitest-environment jsdom

import type { SearchResult } from 'types/api/search';

import * as searchMock from 'mocks/search/index';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, wrapper } from 'vitest/lib';

vi.hoisted(() => {
  window.__envs = {
    ...window.__envs,
    NEXT_PUBLIC_API_HOST: window.location.hostname,
    NEXT_PUBLIC_API_PROTOCOL: window.location.protocol.replace(':', ''),
  };
});

import { useSearchRedirect } from './utils';

const PAX_ADDRESS = 'pax1zg69v7ys40x77y352eufp27daufrg4nc0aa6dg';
const KERNEL_ACCOUNT = 'b5c3f1a0d29e4867b1f0a4c73d5e9182ac6b70d34f81e2905c7ab6134de8f072';
const KERNEL_KEY = '9f2c1b4d5e6a7b8c9d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e';
const LAYERX_DID = `did:layerx:${ KERNEL_KEY }`;
const EVM_ADDRESS = '0x1234567890abcdef1234567890abcdef12345678';
const TX_HASH = `0x${ KERNEL_ACCOUNT }`;

const responseInit = {
  headers: {
    'Content-Type': 'application/json',
  },
};

const boundAccount: SearchResult = {
  items: [ searchMock.token1, searchMock.address1 ],
  next_page_params: null,
};

const nothing: SearchResult = {
  items: [],
  next_page_params: null,
};

const accountRoute = {
  pathname: '/paxeer-x/account/[hash]',
  query: { hash: searchMock.address1.address_hash },
};

beforeEach(() => {
  fetchMock.resetMocks();
});

describe('useSearchRedirect', () => {
  it.each([
    [ 'a pax address', PAX_ADDRESS ],
    [ 'a decentralised identifier', LAYERX_DID ],
    [ 'a kernel account id', KERNEL_ACCOUNT ],
    [ 'a bare kernel key', KERNEL_KEY ],
  ])('resolves %s to the unified account route of the bound address', async(_name, term) => {
    fetchMock.mockResponse(JSON.stringify(boundAccount), responseInit);

    const { result } = renderHook(() => useSearchRedirect(), { wrapper });
    const redirectRoute = await result.current(term, true);

    expect(redirectRoute).toEqual(accountRoute);
    expect(fetchMock.mock.calls).toHaveLength(1);

    const requestUrl = new URL(String(fetchMock.mock.calls[0][0]));

    expect(requestUrl.pathname.endsWith('/api/v2/search')).toBe(true);
    expect(requestUrl.searchParams.get('q')).toBe(term);
  });

  it('falls through to the search results route when the identity resolves to no address', async() => {
    fetchMock.mockResponse(JSON.stringify(nothing), responseInit);

    const { result } = renderHook(() => useSearchRedirect(), { wrapper });
    const redirectRoute = await result.current(LAYERX_DID, true);

    expect(redirectRoute).toEqual({ pathname: '/search-results', query: { q: LAYERX_DID, redirect: 'true' } });
    expect(fetchMock.mock.calls).toHaveLength(1);
  });

  it('falls through to the search results route when the search path fails', async() => {
    fetchMock.mockResponse(JSON.stringify({ message: 'Not found' }), { ...responseInit, status: 404 });

    const { result } = renderHook(() => useSearchRedirect(), { wrapper });
    const redirectRoute = await result.current(PAX_ADDRESS, false);

    expect(redirectRoute).toEqual({ pathname: '/search-results', query: { q: PAX_ADDRESS, redirect: 'false' } });
  });

  it('keeps the search results route for an EVM address and a transaction hash without querying the search path', async() => {
    const { result } = renderHook(() => useSearchRedirect(), { wrapper });

    await expect(result.current(EVM_ADDRESS, true))
      .resolves.toEqual({ pathname: '/search-results', query: { q: EVM_ADDRESS, redirect: 'true' } });
    await expect(result.current(TX_HASH, true))
      .resolves.toEqual({ pathname: '/search-results', query: { q: TX_HASH, redirect: 'true' } });
    await expect(result.current('vitalik.eth', false))
      .resolves.toEqual({ pathname: '/search-results', query: { q: 'vitalik.eth', redirect: 'false' } });

    expect(fetchMock.mock.calls).toHaveLength(0);
  });
});
