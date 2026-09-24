// @vitest-environment jsdom

import { useRouter } from 'next/router';
import type React from 'react';

import * as addressMock from 'mocks/address/address';
import type { Mock } from 'vitest';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, wrapper, act } from 'vitest/lib';
import flushPromises from 'vitest/utils/flushPromises';

const { mockScrollToTop, mockRouterPush } = vi.hoisted(() => ({
  mockScrollToTop: vi.fn(),
  mockRouterPush: vi.fn(() => Promise.resolve(true)),
}));

vi.mock('next/router', () => ({
  useRouter: vi.fn(() => ({
    query: {},
    push: mockRouterPush,
  })),
}));
vi.mock('react-scroll', () => ({ animateScroll: { scrollToTop: mockScrollToTop } }));

import type { Params, QueryWithPagesResult } from './useQueryWithPages';
import useQueryWithPages from './useQueryWithPages';

const mockUseRouter = useRouter as Mock<typeof useRouter>;

export const router = {
  query: {},
  push: mockRouterPush,
  pathname: '/' as const,
} as unknown as ReturnType<typeof useRouter>;

const responses = {
  page_empty: {
    items: [],
    next_page_params: null,
  },
  page_1: {
    items: [ { hash: '11' }, { hash: '12' } ],
    next_page_params: {
      block_number: 11,
      index: 12,
      items_count: 13,
    },
  },
  page_2: {
    items: [ { hash: '21' }, { hash: '22' } ],
    next_page_params: {
      block_number: 21,
      index: 22,
      items_count: 23,
    },
  },
  page_3: {
    items: [ { hash: '31' }, { hash: '32' } ],
    next_page_params: null,
  },
  page_filtered: {
    items: [ { hash: '41' }, { hash: '42' } ],
    next_page_params: {
      block_number: 41,
      index: 42,
      items_count: 43,
    },
  },
  page_sorted: {
    items: [ { hash: '61' }, { hash: '62' } ],
    next_page_params: null,
  },
};

beforeEach(() => {
  fetchMock.resetMocks();
  mockScrollToTop.mockClear();
  mockRouterPush.mockClear();
});

const responseInit = {
  headers: {
    'Content-Type': 'application/json',
  },
};

it('returns correct data if there is only one page', async() => {
  const params: Params<'general:address_txs'> = {
    resourceName: 'general:address_txs',
    pathParams: { hash: addressMock.hash },
  };
  fetchMock.mockResponse(JSON.stringify(responses.page_empty), responseInit);
  mockUseRouter.mockReturnValue(router);

  const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
  await waitForApiResponse();

  expect(result.current.data).toEqual(responses.page_empty);
  expect(result.current.pagination).toMatchObject({
    page: 1,
    canGoBackwards: false,
    hasNextPage: false,
    isLoading: false,
    isVisible: false,
    hasPages: false,
  });
});

describe('if there are multiple pages', () => {
  const params: Params<'general:address_txs'> = {
    resourceName: 'general:address_txs',
    pathParams: { hash: addressMock.hash },
  };

  it('return correct data for the first page', async() => {
    fetchMock.mockResponse(JSON.stringify(responses.page_1), responseInit);
    mockUseRouter.mockReturnValue(router);

    const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    expect(result.current.data).toEqual(responses.page_1);
    expect(result.current.pagination).toMatchObject({
      page: 1,
      canGoBackwards: false,
      hasNextPage: true,
      isLoading: false,
      isVisible: true,
    });
  });

  describe('correctly navigates forward and backward', () => {
    let result: {
      current: QueryWithPagesResult<'general:address_txs'>;
    };

    beforeEach(async() => {
      mockUseRouter.mockReturnValue({ ...router, pathname: '/blocks' });

      fetchMock.once(JSON.stringify(responses.page_1), responseInit);
      fetchMock.once(JSON.stringify(responses.page_2), responseInit);
      fetchMock.once(JSON.stringify(responses.page_3), responseInit);
      fetchMock.once(JSON.stringify(responses.page_1), responseInit);

      // INITIAL LOAD
      const { result: r } = renderHook(() => useQueryWithPages(params), { wrapper });
      result = r;
      await waitForApiResponse();
    });

    it('from page 1 to page 2', async() => {
      await act(() => {
        result.current.pagination.onNextPageClick();
      });
      await waitForApiResponse();

      expect(result.current.data).toEqual(responses.page_2);
      expect(result.current.pagination).toMatchObject({
        page: 2,
        canGoBackwards: true,
        hasNextPage: true,
        isLoading: false,
        isVisible: true,
        hasPages: true,
      });

      expect(mockRouterPush).toHaveBeenCalledTimes(1);
      expect(mockRouterPush).toHaveBeenLastCalledWith(
        {
          pathname: '/blocks',
          query: {
            next_page_params: encodeURIComponent(JSON.stringify(responses.page_1.next_page_params)),
            page: '2',
          },
        },
        undefined,
        { shallow: true },
      );

      expect(mockScrollToTop).toHaveBeenCalledTimes(1);
      expect(mockScrollToTop).toHaveBeenLastCalledWith({ duration: 0 });
    });

    it('from page 2 to page 3', async() => {
      await act(async() => {
        result.current.pagination.onNextPageClick();
      });
      await waitForApiResponse();

      await act(async() => {
        result.current.pagination.onNextPageClick();
      });
      await waitForApiResponse();

      expect(result.current.data).toEqual(responses.page_3);
      expect(result.current.pagination).toMatchObject({
        page: 3,
        canGoBackwards: true,
        hasNextPage: false,
        isLoading: false,
        isVisible: true,
        hasPages: true,
      });

      expect(mockRouterPush).toHaveBeenCalledTimes(2);
      expect(mockRouterPush).toHaveBeenLastCalledWith(
        {
          pathname: '/blocks',
          query: {
            next_page_params: encodeURIComponent(JSON.stringify(responses.page_2.next_page_params)),
            page: '3',
          },
        },
        undefined,
        { shallow: true },
      );

      expect(mockScrollToTop).toHaveBeenCalledTimes(2);
      expect(mockScrollToTop).toHaveBeenLastCalledWith({ duration: 0 });
    });

    it('from page 3 to page 2', async() => {

      await act(() => {
        result.current.pagination.onNextPageClick();
      });
      await waitForApiResponse();

      await act(() => {
        result.current.pagination.onNextPageClick();
      });
      await waitForApiResponse();

      await act(() => {
        result.current.pagination.onPrevPageClick();
      });
      await waitForApiResponse();

      expect(result.current.data).toEqual(responses.page_2);
      expect(result.current.pagination).toMatchObject({
        page: 2,
        canGoBackwards: true,
        hasNextPage: true,
        isLoading: false,
        isVisible: true,
        hasPages: true,
      });

      expect(mockRouterPush).toHaveBeenCalledTimes(3);
      expect(mockRouterPush).toHaveBeenLastCalledWith(
        {
          pathname: '/blocks',
          query: {
            next_page_params: encodeURIComponent(JSON.stringify(responses.page_1.next_page_params)),
            page: '2',
          },
        },
        undefined,
        { shallow: true },
      );

      expect(mockScrollToTop).toHaveBeenCalledTimes(3);
      expect(mockScrollToTop).toHaveBeenLastCalledWith({ duration: 0 });
    });

    it('from page 2 to page 1', async() => {
      await act(() => {
        result.current.pagination.onNextPageClick();
      });
      await waitForApiResponse();

      await act(() => {
        result.current.pagination.onNextPageClick();
      });
      await waitForApiResponse();

      await act(() => {
        result.current.pagination.onPrevPageClick();
      });
      await waitForApiResponse();

      await act(() => {
        result.current.pagination.onPrevPageClick();
      });
      await waitForApiResponse();

      expect(result.current.data).toEqual(responses.page_1);
      expect(result.current.pagination).toMatchObject({
        page: 1,
        canGoBackwards: false,
        hasNextPage: true,
        isLoading: false,
        isVisible: true,
        hasPages: true,
      });

      expect(mockRouterPush).toHaveBeenCalledTimes(4);
      expect(mockRouterPush).toHaveBeenLastCalledWith(
        {
          pathname: '/blocks',
          query: {},
        },
        undefined,
        { shallow: true },
      );

      expect(mockScrollToTop).toHaveBeenCalledTimes(4);
      expect(mockScrollToTop).toHaveBeenLastCalledWith({ duration: 0 });
    });
  });

  it('correctly resets the page', async() => {
    mockUseRouter.mockReturnValue({ ...router, pathname: '/blocks' });

    fetchMock.once(JSON.stringify(responses.page_1), responseInit);
    fetchMock.once(JSON.stringify(responses.page_2), responseInit);
    fetchMock.once(JSON.stringify(responses.page_3), responseInit);
    fetchMock.once(JSON.stringify(responses.page_1), responseInit);

    const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    await act(async() => {
      result.current.pagination.onNextPageClick();
    });
    await waitForApiResponse();

    await act(async() => {
      result.current.pagination.onNextPageClick();
    });
    await waitForApiResponse();

    await act(async() => {
      result.current.pagination.resetPage();
    });
    await waitForApiResponse();

    expect(result.current.data).toEqual(responses.page_1);
    expect(result.current.pagination).toMatchObject({
      page: 1,
      canGoBackwards: false,
      hasNextPage: true,
      isLoading: false,
      isVisible: true,
      hasPages: true,
    });

    expect(mockRouterPush).toHaveBeenCalledTimes(3);
    expect(mockRouterPush).toHaveBeenLastCalledWith(
      {
        pathname: '/blocks',
        query: {},
      },
      undefined,
      { shallow: true },
    );

    expect(mockScrollToTop).toHaveBeenCalledTimes(3);
    expect(mockScrollToTop).toHaveBeenLastCalledWith({ duration: 0 });
  });

  it('when navigates between pages can scroll to custom element', async() => {
    const scrollRef = {
      current: {
        scrollIntoView: vi.fn(),
      },
    };
    const params: Params<'general:address_txs'> = {
      resourceName: 'general:address_txs',
      pathParams: { hash: addressMock.hash },
      scrollRef: scrollRef as unknown as React.RefObject<HTMLDivElement>,
    };
    fetchMock.once(JSON.stringify(responses.page_1), responseInit);
    fetchMock.once(JSON.stringify(responses.page_2), responseInit);
    mockUseRouter.mockReturnValue(router);

    const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    await act(async() => {
      result.current.pagination.onNextPageClick();
    });
    await waitForApiResponse();

    expect(scrollRef.current.scrollIntoView).toHaveBeenCalledTimes(1);
    expect(scrollRef.current.scrollIntoView).toHaveBeenCalledWith(true);
  });
});

describe('if there is page query param in URL', () => {
  it('sets this param as the page number', async() => {
    mockUseRouter.mockReturnValue({ ...router, query: { page: '3' } } as unknown as ReturnType<typeof useRouter>);

    const params: Params<'general:address_txs'> = {
      resourceName: 'general:address_txs',
      pathParams: { hash: addressMock.hash },
    };
    fetchMock.mockResponse(JSON.stringify(responses.page_empty), responseInit);

    const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    expect(result.current.data).toEqual(responses.page_empty);
    expect(result.current.pagination).toMatchObject({
      page: 3,
      canGoBackwards: false,
      hasNextPage: false,
      isLoading: false,
      isVisible: true,
      hasPages: true,
    });
  });

  it('correctly navigates to the following pages', async() => {
    mockUseRouter.mockReturnValue({ ...router, pathname: '/blocks', query: { page: '2' } });

    const params: Params<'general:address_txs'> = {
      resourceName: 'general:address_txs',
      pathParams: { hash: addressMock.hash },
    };
    fetchMock.once(JSON.stringify(responses.page_2), responseInit);
    fetchMock.once(JSON.stringify(responses.page_3), responseInit);

    const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    await act(async() => {
      result.current.pagination.onNextPageClick();
    });
    await waitForApiResponse();

    expect(result.current.data).toEqual(responses.page_3);
    expect(result.current.pagination).toMatchObject({
      page: 3,
      canGoBackwards: true,
      hasNextPage: false,
      isLoading: false,
      isVisible: true,
      hasPages: true,
    });

    expect(mockRouterPush).toHaveBeenCalledTimes(1);
    expect(mockRouterPush).toHaveBeenLastCalledWith(
      {
        pathname: '/blocks',
        query: {
          next_page_params: encodeURIComponent(JSON.stringify(responses.page_2.next_page_params)),
          page: '3',
        },
      },
      undefined,
      { shallow: true },
    );
  });
});

describe('queries with filters', () => {
  it('reset page, keep sorting when filter is changed', async() => {
    mockUseRouter.mockReturnValue({ ...router, pathname: '/blocks', query: { foo: 'bar', sort: 'val-desc' } });

    const params: Params<'general:address_txs'> = {
      resourceName: 'general:address_txs',
      pathParams: { hash: addressMock.hash },

      // @ts-ignore:
      sorting: { sort: 'val-desc' },
    };
    fetchMock.once(JSON.stringify(responses.page_1), responseInit);
    fetchMock.once(JSON.stringify(responses.page_2), responseInit);
    fetchMock.once(JSON.stringify(responses.page_filtered), responseInit);

    const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    await act(async() => {
      result.current.pagination.onNextPageClick();
    });
    await waitForApiResponse();

    await act(async() => {
      result.current.onFilterChange({ filter: 'from' });
    });
    await waitForApiResponse();

    expect(result.current.data).toEqual(responses.page_filtered);
    expect(result.current.pagination).toMatchObject({
      page: 1,
      canGoBackwards: false,
      hasNextPage: true,
      isLoading: false,
      isVisible: true,
      hasPages: false,
    });

    expect(mockRouterPush).toHaveBeenCalledTimes(2);
    expect(mockRouterPush).toHaveBeenLastCalledWith(
      {
        pathname: '/blocks',
        query: { filter: 'from', foo: 'bar', sort: 'val-desc' },
      },
      undefined,
      { shallow: true },
    );

    expect(mockScrollToTop).toHaveBeenCalledTimes(2);
    expect(mockScrollToTop).toHaveBeenLastCalledWith({ duration: 0 });
  });

  it('saves filter params in query when navigating between pages', async() => {
    mockUseRouter.mockReturnValue({ ...router, pathname: '/blocks', query: { filter: 'from', foo: 'bar' } });

    const params: Params<'general:address_txs'> = {
      resourceName: 'general:address_txs',
      pathParams: { hash: addressMock.hash },
    };
    fetchMock.once(JSON.stringify(responses.page_1), responseInit);
    fetchMock.once(JSON.stringify(responses.page_2), responseInit);

    const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    await act(async() => {
      result.current.pagination.onNextPageClick();
    });
    await waitForApiResponse();

    expect(mockRouterPush).toHaveBeenCalledTimes(1);
    expect(mockRouterPush).toHaveBeenLastCalledWith(
      {
        pathname: '/blocks',
        query: {
          filter: 'from',
          foo: 'bar',
          next_page_params: encodeURIComponent(JSON.stringify(responses.page_1.next_page_params)),
          page: '2',
        },
      },
      undefined,
      { shallow: true },
    );
  });
});

describe('queries with sorting', () => {
  it('reset page, save filter when sorting is changed', async() => {
    mockUseRouter.mockReturnValue({ ...router, pathname: '/blocks', query: { foo: 'bar', filter: 'from' } });

    const params: Params<'general:address_txs'> = {
      resourceName: 'general:address_txs',
      pathParams: { hash: addressMock.hash },
      filters: { filter: 'from' },
    };
    fetchMock.once(JSON.stringify(responses.page_1), responseInit);
    fetchMock.once(JSON.stringify(responses.page_2), responseInit);
    fetchMock.once(JSON.stringify(responses.page_sorted), responseInit);

    const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    await act(async() => {
      result.current.pagination.onNextPageClick();
    });
    await waitForApiResponse();

    await act(async() => {

      // @ts-ignore:
      result.current.onSortingChange({ sort: 'val-desc' });
    });
    await waitForApiResponse();

    expect(result.current.data).toEqual(responses.page_sorted);
    expect(result.current.pagination).toMatchObject({
      page: 1,
      canGoBackwards: false,
      hasNextPage: false,
      isLoading: false,
      isVisible: false,
      hasPages: false,
    });

    expect(mockRouterPush).toHaveBeenCalledTimes(2);
    expect(mockRouterPush).toHaveBeenLastCalledWith(
      {
        pathname: '/blocks',
        query: { filter: 'from', foo: 'bar', sort: 'val-desc' },
      },
      undefined,
      { shallow: true },
    );

    expect(mockScrollToTop).toHaveBeenCalledTimes(2);
    expect(mockScrollToTop).toHaveBeenLastCalledWith({ duration: 0 });
  });

  it('saves sorting params in query when navigating between pages', async() => {
    mockUseRouter.mockReturnValue({ ...router, pathname: '/blocks', query: { foo: 'bar', sort: 'val-desc' } });

    const params: Params<'general:address_txs'> = {
      resourceName: 'general:address_txs',
      pathParams: { hash: addressMock.hash },

      // @ts-ignore:
      sorting: { sort: 'val-desc' },
    };
    fetchMock.once(JSON.stringify(responses.page_1), responseInit);
    fetchMock.once(JSON.stringify(responses.page_2), responseInit);

    const { result } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    await act(async() => {
      result.current.pagination.onNextPageClick();
    });
    await waitForApiResponse();

    expect(mockRouterPush).toHaveBeenCalledTimes(1);
    expect(mockRouterPush).toHaveBeenLastCalledWith(
      {
        pathname: '/blocks',
        query: {
          sort: 'val-desc',
          foo: 'bar',
          next_page_params: encodeURIComponent(JSON.stringify(responses.page_1.next_page_params)),
          page: '2',
        },
      },
      undefined,
      { shallow: true },
    );
  });
});

describe('router query changes', () => {
  it('refetches correct page when page number changes in URL', async() => {
    const router = {
      pathname: '/blocks' as const,
      push: mockRouterPush,
      query: {
        page: '3',
        next_page_params: encodeURIComponent(JSON.stringify(responses.page_2.next_page_params)),
      },
    } as unknown as ReturnType<typeof useRouter>;
    mockUseRouter.mockReturnValue(router);

    const params: Params<'general:address_txs'> = {
      resourceName: 'general:address_txs',
      pathParams: { hash: addressMock.hash },
    };

    fetchMock.once(JSON.stringify(responses.page_3), responseInit);
    fetchMock.once(JSON.stringify(responses.page_2), responseInit);

    const { result, rerender } = renderHook(() => useQueryWithPages(params), { wrapper });
    await waitForApiResponse();

    expect(result.current.data).toEqual(responses.page_3);
    expect(result.current.pagination.page).toBe(3);

    // Simulate URL change to page 2
    mockUseRouter.mockReturnValue({
      ...router,
      query: {
        page: '2',
        next_page_params: encodeURIComponent(JSON.stringify(responses.page_1.next_page_params)),
      } as never,
    });

    rerender();
    await waitForApiResponse();

    expect(result.current.data).toEqual(responses.page_2);
    expect(result.current.pagination).toMatchObject({
      page: 2,
      canGoBackwards: false,
      hasNextPage: true,
      isLoading: false,
      isVisible: true,
    });
  });
});

async function waitForApiResponse() {
  await flushPromises();
  await act(flushPromises);
}
