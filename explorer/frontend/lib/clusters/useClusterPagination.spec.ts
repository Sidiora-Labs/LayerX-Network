// @vitest-environment jsdom

import { useRouter } from 'next/router';

import getQueryParamString from 'lib/router/getQueryParamString';
import { useQueryParams } from 'lib/router/useQueryParams';
import type { Mock } from 'vitest';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, act } from 'vitest/lib';

import { useClusterPagination } from '../clusters/useClusterPagination';

vi.mock('next/router', () => ({
  useRouter: vi.fn(),
}));
vi.mock('lib/router/useQueryParams');
vi.mock('lib/router/getQueryParamString');

const mockUseRouter = useRouter as Mock<typeof useRouter>;
const mockUseQueryParams = useQueryParams as Mock<typeof useQueryParams>;
const mockGetQueryParamString = getQueryParamString as Mock<typeof getQueryParamString>;

describe('useClusterPagination', () => {
  const mockUpdateQuery = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    mockUseRouter.mockReturnValue({
      query: {},
    } as unknown as ReturnType<typeof useRouter>);
    mockUseQueryParams.mockReturnValue({
      updateQuery: mockUpdateQuery,
    });
  });

  describe('page calculation', () => {
    it('should default to page 1 when no page param', () => {
      mockGetQueryParamString.mockReturnValue('');

      const { result } = renderHook(() => useClusterPagination(true, false));

      expect(result.current.page).toBe(1);
      expect(result.current.pagination.page).toBe(1);
    });

    it('should parse page from query param', () => {
      mockGetQueryParamString.mockReturnValue('3');

      const { result } = renderHook(() => useClusterPagination(true, false));

      expect(result.current.page).toBe(3);
      expect(result.current.pagination.page).toBe(3);
    });

    it('should handle invalid page param gracefully', () => {
      mockGetQueryParamString.mockReturnValue('invalid');

      const { result } = renderHook(() => useClusterPagination(true, false));

      expect(result.current.page).toBeNaN();
    });
  });

  describe('navigation functions', () => {
    beforeEach(() => {
      mockGetQueryParamString.mockReturnValue('2');
    });

    it('should increment page on next click', () => {
      const { result } = renderHook(() => useClusterPagination(true, false));

      act(() => {
        result.current.pagination.onNextPageClick();
      });

      expect(mockUpdateQuery).toHaveBeenCalledWith({ page: '3' });
    });

    it('should decrement page on prev click', () => {
      const { result } = renderHook(() => useClusterPagination(true, false));

      act(() => {
        result.current.pagination.onPrevPageClick();
      });

      expect(mockUpdateQuery).toHaveBeenCalledWith({ page: undefined });
    });

    it('should handle prev click from page 3', () => {
      mockGetQueryParamString.mockReturnValue('3');
      const { result } = renderHook(() => useClusterPagination(true, false));

      act(() => {
        result.current.pagination.onPrevPageClick();
      });

      expect(mockUpdateQuery).toHaveBeenCalledWith({ page: '2' });
    });

    it('should reset page to undefined', () => {
      const { result } = renderHook(() => useClusterPagination(true, false));

      act(() => {
        result.current.pagination.resetPage();
      });

      expect(mockUpdateQuery).toHaveBeenCalledWith({ page: undefined });
    });
  });

  describe('pagination state', () => {
    it('should set hasPages true when page > 1', () => {
      mockGetQueryParamString.mockReturnValue('2');

      const { result } = renderHook(() => useClusterPagination(true, false));

      expect(result.current.pagination.hasPages).toBe(true);
    });

    it('should set hasPages false when page = 1', () => {
      mockGetQueryParamString.mockReturnValue('1');

      const { result } = renderHook(() => useClusterPagination(true, false));

      expect(result.current.pagination.hasPages).toBe(false);
    });

    it('should set canGoBackwards true when page > 1', () => {
      mockGetQueryParamString.mockReturnValue('2');

      const { result } = renderHook(() => useClusterPagination(true, false));

      expect(result.current.pagination.canGoBackwards).toBe(true);
    });

    it('should set canGoBackwards false when page = 1', () => {
      mockGetQueryParamString.mockReturnValue('1');

      const { result } = renderHook(() => useClusterPagination(true, false));

      expect(result.current.pagination.canGoBackwards).toBe(false);
    });

    it('should pass through hasNextPage prop', () => {
      mockGetQueryParamString.mockReturnValue('1');

      const { result } = renderHook(() => useClusterPagination(false, false));

      expect(result.current.pagination.hasNextPage).toBe(false);
    });

    it('should pass through isLoading prop', () => {
      mockGetQueryParamString.mockReturnValue('1');

      const { result } = renderHook(() => useClusterPagination(true, true));

      expect(result.current.pagination.isLoading).toBe(true);
    });
  });

  describe('pagination visibility', () => {
    it('should be visible when page > 1', () => {
      mockGetQueryParamString.mockReturnValue('2');

      const { result } = renderHook(() => useClusterPagination(false, false));

      expect(result.current.pagination.isVisible).toBe(true);
    });

    it('should be visible when hasNextPage is true', () => {
      mockGetQueryParamString.mockReturnValue('1');

      const { result } = renderHook(() => useClusterPagination(true, false));

      expect(result.current.pagination.isVisible).toBe(true);
    });

    it('should not be visible when page = 1 and no next page', () => {
      mockGetQueryParamString.mockReturnValue('1');

      const { result } = renderHook(() => useClusterPagination(false, false));

      expect(result.current.pagination.isVisible).toBe(false);
    });
  });

  describe('function stability', () => {
    it('should not recreate functions when dependencies do not change', () => {
      mockGetQueryParamString.mockReturnValue('2');

      const { result, rerender } = renderHook(() => useClusterPagination(true, false));

      const firstOnNext = result.current.pagination.onNextPageClick;
      const firstOnPrev = result.current.pagination.onPrevPageClick;
      const firstReset = result.current.pagination.resetPage;

      rerender();

      expect(result.current.pagination.onNextPageClick).toBe(firstOnNext);
      expect(result.current.pagination.onPrevPageClick).toBe(firstOnPrev);
      expect(result.current.pagination.resetPage).toBe(firstReset);
    });
  });
});
