import { ClustersOrderBy } from 'types/api/clusters';
import type { ClustersDirectoryObject } from 'types/api/clusters';

import { describe, it, expect } from 'vitest';

import {
  getViewModeOrderBy,
  shouldShowDirectoryView,
  transformLeaderboardData,
  transformAddressDataToDirectory,
  transformFullDirectoryData,
  applyDirectoryPagination,
  calculateHasNextPage,
  isValidViewMode,
  getDefaultViewMode,
  getCurrentDataLength,
} from './pageUtils';

describe('pageUtils', () => {
  describe('getViewModeOrderBy', () => {
    it('should return RANK_ASC for leaderboard view regardless of search', () => {
      expect(getViewModeOrderBy('leaderboard', false)).toBe(ClustersOrderBy.RANK_ASC);
      expect(getViewModeOrderBy('leaderboard', true)).toBe(ClustersOrderBy.RANK_ASC);
    });

    it('should return NAME_ASC for directory view with search term', () => {
      expect(getViewModeOrderBy('directory', true)).toBe(ClustersOrderBy.NAME_ASC);
    });

    it('should return CREATED_AT_DESC for directory view without search term', () => {
      expect(getViewModeOrderBy('directory', false)).toBe(ClustersOrderBy.CREATED_AT_DESC);
    });
  });

  describe('shouldShowDirectoryView', () => {
    it('should return true for directory view mode', () => {
      expect(shouldShowDirectoryView('directory', false)).toBe(true);
      expect(shouldShowDirectoryView('directory', true)).toBe(true);
    });

    it('should return true for leaderboard mode with search term', () => {
      expect(shouldShowDirectoryView('leaderboard', true)).toBe(true);
    });

    it('should return false for leaderboard mode without search term', () => {
      expect(shouldShowDirectoryView('leaderboard', false)).toBe(false);
    });
  });

  describe('transformLeaderboardData', () => {
    const mockLeaderboardData = {
      result: {
        data: [
          { name: 'cluster1', rank: 1 },
          { name: 'cluster2', rank: 2 },
        ],
      },
    };

    it('should return empty array when showDirectoryView is true', () => {
      expect(transformLeaderboardData(mockLeaderboardData, true)).toEqual([]);
    });

    it('should return empty array when data is null', () => {
      expect(transformLeaderboardData(null, false)).toEqual([]);
    });

    it('should return transformed data when valid', () => {
      const result = transformLeaderboardData(mockLeaderboardData, false);
      expect(result).toEqual([
        { name: 'cluster1', rank: 1 },
        { name: 'cluster2', rank: 2 },
      ]);
    });

    it('should return empty array for invalid data structure', () => {
      expect(transformLeaderboardData({ invalid: 'data' }, false)).toEqual([]);
      expect(transformLeaderboardData({ result: { data: 'not-array' } }, false)).toEqual([]);
    });
  });

  describe('transformAddressDataToDirectory', () => {
    const mockAddressData = [
      {
        name: 'test-cluster',
        isTestnet: false,
        createdAt: '2023-01-01',
        owner: '0x123',
        totalWeiAmount: '1000',
        updatedAt: '2023-01-02',
        updatedBy: '0x456',
        clusterId: 'test-cluster-id',
        expiresAt: '2024-01-01',
      },
    ];

    const mockClusterDetails = {
      result: {
        data: {
          wallets: [
            { chainIds: [ '1', '137' ] },
            { chainIds: [ '1', '56' ] },
          ],
        },
      },
    };

    it('should transform address data with unique chain IDs', () => {
      const result = transformAddressDataToDirectory(mockAddressData, mockClusterDetails);

      expect(result).toHaveLength(1);
      expect(result[0]).toEqual({
        name: 'test-cluster',
        isTestnet: false,
        createdAt: '2023-01-01',
        owner: '0x123',
        totalWeiAmount: '1000',
        updatedAt: '2023-01-02',
        updatedBy: '0x456',
        chainIds: [ '1', '137', '56' ],
      });
    });

    it('should handle missing cluster details', () => {
      const result = transformAddressDataToDirectory(mockAddressData, null);

      expect(result[0].chainIds).toEqual([]);
    });

    it('should handle empty wallets array', () => {
      const emptyDetails = { result: { data: { wallets: [] } } };
      const result = transformAddressDataToDirectory(mockAddressData, emptyDetails);

      expect(result[0].chainIds).toEqual([]);
    });
  });

  describe('transformFullDirectoryData', () => {
    it('should return empty array when showDirectoryView is false', () => {
      const result = transformFullDirectoryData({}, {}, 'address', false);
      expect(result).toEqual([]);
    });

    it('should return empty array when data is null', () => {
      const result = transformFullDirectoryData(null, {}, 'address', true);
      expect(result).toEqual([]);
    });

    it('should transform address-type data', () => {
      const mockData = {
        result: {
          data: [ { name: 'cluster1', owner: '0x123' } ],
        },
      };
      const mockDetails = {
        result: { data: { wallets: [ { chainIds: [ '1' ] } ] } },
      };

      const result = transformFullDirectoryData(mockData, mockDetails, 'address', true);
      expect(result).toHaveLength(1);
      expect(result[0].name).toBe('cluster1');
    });

    it('should transform cluster_name-type data', () => {
      const mockData = {
        result: {
          data: {
            items: [ { name: 'cluster1' }, { name: 'cluster2' } ],
          },
        },
      };

      const result = transformFullDirectoryData(mockData, {}, 'cluster_name', true);
      expect(result).toEqual([ { name: 'cluster1' }, { name: 'cluster2' } ]);
    });
  });

  describe('applyDirectoryPagination', () => {
    const mockData = Array.from({ length: 100 }, (_, i) => ({ name: `cluster${ i }` })) as Array<ClustersDirectoryObject>;

    it('should apply pagination for address input type', () => {
      const result = applyDirectoryPagination(mockData, 'address', 2, 20);
      expect(result).toHaveLength(20);
      expect(result[0].name).toBe('cluster20');
      expect(result[19].name).toBe('cluster39');
    });

    it('should return all data for cluster_name input type', () => {
      const result = applyDirectoryPagination(mockData, 'cluster_name', 2, 20);
      expect(result).toHaveLength(100);
      expect(result[0].name).toBe('cluster0');
    });

    it('should handle last page correctly', () => {
      const result = applyDirectoryPagination(mockData, 'address', 5, 20);
      expect(result).toHaveLength(20);
      expect(result[0].name).toBe('cluster80');
    });

    it('should handle page beyond data length', () => {
      const result = applyDirectoryPagination(mockData, 'address', 10, 20);
      expect(result).toHaveLength(0);
    });
  });

  describe('calculateHasNextPage', () => {
    const mockDirectoryData = {
      result: {
        data: {
          total: 100,
        },
      },
    };

    it('should return true for address type with more data', () => {
      const result = calculateHasNextPage(
        {},
        0,
        200,
        true,
        'address',
        2,
        false,
        50,
      );
      expect(result).toBe(true);
    });

    it('should return false for address type at end', () => {
      const result = calculateHasNextPage(
        {},
        0,
        100,
        true,
        'address',
        2,
        false,
        50,
      );
      expect(result).toBe(false);
    });

    it('should return false for cluster_name type with search term', () => {
      const result = calculateHasNextPage(
        mockDirectoryData,
        0,
        0,
        true,
        'cluster_name',
        1,
        true,
        50,
      );
      expect(result).toBe(false);
    });

    it('should return true for cluster_name type without search and more pages', () => {
      const result = calculateHasNextPage(
        mockDirectoryData,
        0,
        0,
        true,
        'cluster_name',
        1,
        false,
        50,
      );
      expect(result).toBe(true);
    });

    it('should return true for leaderboard with full page', () => {
      const result = calculateHasNextPage(
        {},
        50,
        0,
        false,
        'cluster_name',
        1,
        false,
        50,
      );
      expect(result).toBe(true);
    });

    it('should return false for leaderboard with partial page', () => {
      const result = calculateHasNextPage(
        {},
        25,
        0,
        false,
        'cluster_name',
        1,
        false,
        50,
      );
      expect(result).toBe(false);
    });
  });

  describe('isValidViewMode', () => {
    it('should return true for valid view modes', () => {
      expect(isValidViewMode('leaderboard')).toBe(true);
      expect(isValidViewMode('directory')).toBe(true);
    });

    it('should return false for invalid view modes', () => {
      expect(isValidViewMode('invalid')).toBe(false);
      expect(isValidViewMode('')).toBe(false);
      expect(isValidViewMode('grid')).toBe(false);
    });
  });

  describe('getDefaultViewMode', () => {
    it('should return directory as default', () => {
      expect(getDefaultViewMode()).toBe('directory');
    });
  });

  describe('getCurrentDataLength', () => {
    it('should return directory data length when showing directory view', () => {
      expect(getCurrentDataLength(true, 25, 50)).toBe(25);
    });

    it('should return leaderboard data length when showing leaderboard view', () => {
      expect(getCurrentDataLength(false, 25, 50)).toBe(50);
    });

    it('should handle zero lengths', () => {
      expect(getCurrentDataLength(true, 0, 10)).toBe(0);
      expect(getCurrentDataLength(false, 10, 0)).toBe(0);
    });
  });
});
