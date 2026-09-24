// @vitest-environment jsdom

import { useRouter } from 'next/router';

import type { Mock } from 'vitest';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook } from 'vitest/lib';

import { useClusterSearch } from './useClusterSearch';

vi.mock('next/router', () => ({
  useRouter: vi.fn(),
}));

const mockUseRouter = useRouter as Mock<typeof useRouter>;

describe('useClusterSearch', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should return search term from router query', async() => {
    mockUseRouter.mockReturnValue({
      query: { q: 'test-search' },
    } as unknown as ReturnType<typeof useRouter>);

    const { result } = await renderHook(() => useClusterSearch());

    expect(result.current.searchTerm).toBe('test-search');
  });

  it('should debounce search term', async() => {
    mockUseRouter.mockReturnValue({
      query: { q: 'test' },
    } as unknown as ReturnType<typeof useRouter>);

    const { result } = await renderHook(() => useClusterSearch());

    expect(result.current.debouncedSearchTerm).toBe('test');
  });

  it('should handle empty query', async() => {
    mockUseRouter.mockReturnValue({
      query: {},
    } as unknown as ReturnType<typeof useRouter>);

    const { result } = await renderHook(() => useClusterSearch());

    expect(result.current.searchTerm).toBe('');
    expect(result.current.debouncedSearchTerm).toBe('');
  });
});
