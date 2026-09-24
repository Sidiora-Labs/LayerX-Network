// @vitest-environment jsdom

import { renderHook } from '@testing-library/react';

import useApiQuery from 'lib/api/useApiQuery';
import { detectInputType } from 'lib/clusters/detectInputType';
import type { Mock } from 'vitest';
import { describe, it, expect, beforeEach, vi } from 'vitest';

import { useClustersData } from './useClustersData';

vi.mock('lib/api/useApiQuery');
const mockUseApiQuery = useApiQuery as Mock<typeof useApiQuery>;

type MockQueryResult = ReturnType<typeof useApiQuery>;

vi.mock('lib/clusters/detectInputType', () => ({
  detectInputType: vi.fn(),
}));

const mockDetectInputType = detectInputType as Mock<typeof detectInputType>;

describe('useClustersData', () => {
  beforeEach(() => {
    vi.clearAllMocks();

    mockUseApiQuery.mockReturnValue({
      data: null,
      isError: false,
      isPlaceholderData: false,
      isLoading: false,
    } as unknown as MockQueryResult);
  });

  describe('input type detection logic', () => {
    it('should default to cluster_name when no search term provided', () => {
      renderHook(() => useClustersData('', 'leaderboard', 1));

      expect(mockDetectInputType).not.toHaveBeenCalled();
    });

    it('should call detectInputType when search term exists', () => {
      mockDetectInputType.mockReturnValue('address');

      renderHook(() => useClustersData('0x123...', 'directory', 1));

      expect(mockDetectInputType).toHaveBeenCalledWith('0x123...');
    });

    it('should memoize input type calculation', () => {
      mockDetectInputType.mockReturnValue('cluster_name');

      const { rerender } = renderHook(
        ({ searchTerm }) => useClustersData(searchTerm, 'directory', 1),
        { initialProps: { searchTerm: 'example.cluster' } },
      );

      rerender({ searchTerm: 'example.cluster' });

      expect(mockDetectInputType).toHaveBeenCalledTimes(1);
    });
  });

  describe('view mode determination', () => {
    it('should show directory view when viewMode is directory', () => {
      renderHook(() => useClustersData('', 'directory', 1));

      expect(mockUseApiQuery).toHaveBeenCalledWith('clusters:get_directory', expect.any(Object));
    });

    it('should show directory view when search term exists regardless of viewMode', () => {
      mockDetectInputType.mockReturnValue('cluster_name');

      renderHook(() => useClustersData('search', 'leaderboard', 1));

      expect(mockUseApiQuery).toHaveBeenCalledWith('clusters:get_directory', expect.any(Object));
    });

    it('should show leaderboard view when no search term and viewMode is leaderboard', () => {
      renderHook(() => useClustersData('', 'leaderboard', 1));

      expect(mockUseApiQuery).toHaveBeenCalledWith('clusters:get_leaderboard', expect.any(Object));
    });
  });

  describe('API query configuration', () => {
    it('should configure leaderboard query with correct pagination', () => {
      renderHook(() => useClustersData('', 'leaderboard', 3));

      const leaderboardCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_leaderboard',
      );

      expect(leaderboardCall).toBeDefined();
      expect(leaderboardCall?.[1]?.queryParams?.input).toContain('"offset":100');
      expect(leaderboardCall?.[1]?.queryParams?.input).toContain('"limit":50');
      expect(leaderboardCall?.[1]?.queryParams?.input).toContain('"orderBy":"rank-asc"');
    });

    it('should configure directory query with search term', () => {
      mockDetectInputType.mockReturnValue('cluster_name');

      renderHook(() => useClustersData('example', 'directory', 2));

      const directoryCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_directory',
      );

      expect(directoryCall).toBeDefined();
      expect(directoryCall?.[1]?.queryParams?.input).toContain('"offset":50');
      expect(directoryCall?.[1]?.queryParams?.input).toContain('"query":"example"');
    });

    it('should configure address query when input type is address', () => {
      mockDetectInputType.mockReturnValue('address');

      renderHook(() => useClustersData('0x123...', 'directory', 1));

      const addressCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_clusters_by_address',
      );

      expect(addressCall).toBeDefined();
      expect(addressCall?.[1]?.queryParams?.input).toContain('"address":"0x123..."');
    });

    it('should call cluster details query when cluster ID is available', () => {
      mockDetectInputType.mockReturnValue('address');

      mockUseApiQuery.mockImplementation((resource) => {
        if (resource === 'clusters:get_clusters_by_address') {
          return {
            data: {
              result: {
                data: [ { clusterId: 'cluster-123' } ],
              },
            },
            isError: false,
            isPlaceholderData: false,
            isLoading: false,
          } as unknown as MockQueryResult;
        }
        return {
          data: null,
          isError: false,
          isPlaceholderData: false,
          isLoading: false,
        } as unknown as MockQueryResult;
      });

      renderHook(() => useClustersData('0x123...', 'directory', 1));

      const clusterDetailsCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_cluster_by_id',
      );

      expect(clusterDetailsCall).toBeDefined();
      expect(clusterDetailsCall?.[1]?.queryParams?.input).toContain('"id":"cluster-123"');
    });
  });

  describe('dynamic ordering business logic', () => {
    it('should use NAME_ASC ordering when search term exists', () => {
      mockDetectInputType.mockReturnValue('cluster_name');

      renderHook(() => useClustersData('search', 'directory', 1));

      const directoryCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_directory',
      );

      expect(directoryCall?.[1]?.queryParams?.input).toContain('"orderBy":"name-asc"');
    });

    it('should use CREATED_AT_DESC ordering when no search term', () => {
      renderHook(() => useClustersData('', 'directory', 1));

      const directoryCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_directory',
      );

      expect(directoryCall?.[1]?.queryParams?.input).toContain('"orderBy":"createdAt-desc"');
    });

    it('should memoize directory order by logic', () => {
      const { rerender } = renderHook(
        ({ searchTerm }) => useClustersData(searchTerm, 'directory', 1),
        { initialProps: { searchTerm: 'search' } },
      );

      vi.clearAllMocks();

      rerender({ searchTerm: 'search' });

      const expectedCallsPerRender = 4;
      expect(mockUseApiQuery.mock.calls.length).toBe(expectedCallsPerRender);
    });
  });

  describe('query selection logic', () => {
    it('should return data from leaderboard query in leaderboard mode', () => {
      const mockLeaderboardData = { result: { data: [] } };

      mockUseApiQuery.mockImplementation((resource) => {
        if (resource === 'clusters:get_leaderboard') {
          return {
            data: mockLeaderboardData,
            isError: false,
            isPlaceholderData: false,
            isLoading: false,
          } as unknown as MockQueryResult;
        }
        return {
          data: null,
          isError: false,
          isPlaceholderData: false,
          isLoading: false,
        } as unknown as MockQueryResult;
      });

      const { result } = renderHook(() =>
        useClustersData('', 'leaderboard', 1),
      );

      expect(result.current.data).toBe(mockLeaderboardData);
    });

    it('should return data from address query when input type is address', () => {
      mockDetectInputType.mockReturnValue('address');
      const mockAddressData = { result: { data: [] } };

      mockUseApiQuery.mockImplementation((resource) => {
        if (resource === 'clusters:get_clusters_by_address') {
          return {
            data: mockAddressData,
            isError: false,
            isPlaceholderData: false,
            isLoading: false,
          } as unknown as MockQueryResult;
        }
        return {
          data: null,
          isError: false,
          isPlaceholderData: false,
          isLoading: false,
        } as unknown as MockQueryResult;
      });

      const { result } = renderHook(() =>
        useClustersData('0x123...', 'directory', 1),
      );

      expect(result.current.data).toBe(mockAddressData);
    });

    it('should return data from directory query when input type is cluster_name', () => {
      mockDetectInputType.mockReturnValue('cluster_name');
      const mockDirectoryData = { result: { data: { items: [] } } };

      mockUseApiQuery.mockImplementation((resource) => {
        if (resource === 'clusters:get_directory') {
          return {
            data: mockDirectoryData,
            isError: false,
            isPlaceholderData: false,
            isLoading: false,
          } as unknown as MockQueryResult;
        }
        return {
          data: null,
          isError: false,
          isPlaceholderData: false,
          isLoading: false,
        } as unknown as MockQueryResult;
      });

      const { result } = renderHook(() =>
        useClustersData('example', 'directory', 1),
      );

      expect(result.current.data).toBe(mockDirectoryData);
    });
  });

  describe('return value structure', () => {
    it('should return correct data structure with all expected properties', () => {
      const mockData = { result: { data: [] } };
      const mockClusterDetails = { result: { data: { id: 'cluster-123' } } };

      mockUseApiQuery.mockImplementation((resource) => {
        if (resource === 'clusters:get_leaderboard') {
          return {
            data: mockData,
            isError: false,
            isPlaceholderData: false,
            isLoading: false,
          } as unknown as MockQueryResult;
        }
        if (resource === 'clusters:get_cluster_by_id') {
          return {
            data: mockClusterDetails,
            isError: false,
            isPlaceholderData: false,
            isLoading: true,
          } as unknown as MockQueryResult;
        }
        return {
          data: null,
          isError: false,
          isPlaceholderData: false,
          isLoading: false,
        } as unknown as MockQueryResult;
      });

      const { result } = renderHook(() =>
        useClustersData('', 'leaderboard', 1),
      );

      expect(result.current).toEqual({
        data: mockData,
        clusterDetails: mockClusterDetails,
        isError: false,
        isLoading: false,
        isClusterDetailsLoading: true,
      });
    });

    it('should handle error states correctly', () => {
      mockUseApiQuery.mockReturnValue({
        data: null,
        isError: true,
        isPlaceholderData: false,
        isLoading: false,
      } as unknown as MockQueryResult);

      const { result } = renderHook(() =>
        useClustersData('', 'leaderboard', 1),
      );

      expect(result.current.isError).toBe(true);
    });

    it('should handle loading states correctly', () => {
      mockUseApiQuery.mockReturnValue({
        data: null,
        isError: false,
        isPlaceholderData: true,
        isLoading: false,
      } as unknown as MockQueryResult);

      const { result } = renderHook(() =>
        useClustersData('', 'leaderboard', 1),
      );

      expect(result.current.isLoading).toBe(true);
    });
  });

  describe('pagination calculations', () => {
    it('should calculate correct offset for page 1', () => {
      renderHook(() => useClustersData('', 'leaderboard', 1));

      const leaderboardCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_leaderboard',
      );

      expect(leaderboardCall?.[1]?.queryParams?.input).toContain('"offset":0');
    });

    it('should calculate correct offset for page 5', () => {
      renderHook(() => useClustersData('', 'leaderboard', 5));

      const leaderboardCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_leaderboard',
      );

      expect(leaderboardCall?.[1]?.queryParams?.input).toContain('"offset":200');
    });

    it('should consistently use 50 items per page', () => {
      renderHook(() => useClustersData('', 'leaderboard', 1));

      const leaderboardCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_leaderboard',
      );

      expect(leaderboardCall?.[1]?.queryParams?.input).toContain('"limit":50');
    });
  });

  describe('query enabling/disabling logic', () => {
    it('should disable leaderboard query when in directory view', () => {
      renderHook(() => useClustersData('search', 'directory', 1));

      const leaderboardCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_leaderboard',
      );

      expect(leaderboardCall?.[1]?.queryOptions?.enabled).toBe(false);
    });

    it('should disable directory query when input type is address', () => {
      mockDetectInputType.mockReturnValue('address');

      renderHook(() => useClustersData('0x123...', 'directory', 1));

      const directoryCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_directory',
      );

      expect(directoryCall?.[1]?.queryOptions?.enabled).toBe(false);
    });

    it('should disable address query when input type is not address', () => {
      mockDetectInputType.mockReturnValue('cluster_name');

      renderHook(() => useClustersData('example', 'directory', 1));

      const addressCall = mockUseApiQuery.mock.calls.find(call =>
        call[0] === 'clusters:get_clusters_by_address',
      );

      expect(addressCall?.[1]?.queryOptions?.enabled).toBe(false);
    });
  });
});
