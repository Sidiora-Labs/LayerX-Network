import { describe, it, expect } from 'vitest';

import {
  filterOwnedClusters,
  getTotalRecordsDisplay,
  getClusterLabel,
  getClustersToShow,
  getGridRows,
  hasMoreClusters,
  type ClusterData,
} from './clustersUtils';

describe('clustersUtils', () => {
  const mockClusters: Array<ClusterData> = [
    {
      name: 'cluster1',
      owner: '0x1234567890123456789012345678901234567890',
      totalWeiAmount: '1000000000000000000',
      createdAt: '2023-01-01',
      updatedAt: '2023-01-01',
      updatedBy: 'user1',
      isTestnet: false,
      clusterId: 'id1',
      expiresAt: null,
    },
    {
      name: 'cluster2',
      owner: '0xABCDEF1234567890123456789012345678901234',
      totalWeiAmount: '2000000000000000000',
      createdAt: '2023-01-02',
      updatedAt: '2023-01-02',
      updatedBy: 'user2',
      isTestnet: false,
      clusterId: 'id2',
      expiresAt: null,
    },
    {
      name: 'cluster3',
      owner: '0x1234567890123456789012345678901234567890',
      totalWeiAmount: '3000000000000000000',
      createdAt: '2023-01-03',
      updatedAt: '2023-01-03',
      updatedBy: 'user3',
      isTestnet: false,
      clusterId: 'id3',
      expiresAt: null,
    },
  ];

  describe('filterOwnedClusters', () => {
    it('should filter clusters by owner address', () => {
      const result = filterOwnedClusters(mockClusters, '0x1234567890123456789012345678901234567890');

      expect(result).toHaveLength(2);
      expect(result[0].name).toBe('cluster1');
      expect(result[1].name).toBe('cluster3');
    });

    it('should handle case insensitive address matching', () => {
      const result = filterOwnedClusters(mockClusters, '0x1234567890123456789012345678901234567890'.toUpperCase());

      expect(result).toHaveLength(2);
      expect(result[0].name).toBe('cluster1');
      expect(result[1].name).toBe('cluster3');
    });

    it('should return empty array for non-matching address', () => {
      const result = filterOwnedClusters(mockClusters, '0x9999999999999999999999999999999999999999');

      expect(result).toHaveLength(0);
    });

    it('should filter out clusters without owner', () => {
      const clustersWithoutOwner = [
        ...mockClusters,
        {
          name: 'cluster4',
          owner: null as unknown as string,
          totalWeiAmount: '4000000000000000000',
          createdAt: '2023-01-04',
          updatedAt: '2023-01-04',
          updatedBy: 'user4',
          isTestnet: false,
          clusterId: 'id4',
          expiresAt: null,
        },
      ];

      const result = filterOwnedClusters(clustersWithoutOwner, '0x1234567890123456789012345678901234567890');

      expect(result).toHaveLength(2);
    });
  });

  describe('getTotalRecordsDisplay', () => {
    it('should return exact count for numbers 40 and below', () => {
      expect(getTotalRecordsDisplay(1)).toBe('1');
      expect(getTotalRecordsDisplay(10)).toBe('10');
      expect(getTotalRecordsDisplay(40)).toBe('40');
    });

    it('should return "40+" for numbers above 40', () => {
      expect(getTotalRecordsDisplay(41)).toBe('40+');
      expect(getTotalRecordsDisplay(100)).toBe('40+');
      expect(getTotalRecordsDisplay(999)).toBe('40+');
    });

    it('should handle edge case of 0', () => {
      expect(getTotalRecordsDisplay(0)).toBe('0');
    });
  });

  describe('getClusterLabel', () => {
    it('should return singular for count of 1', () => {
      expect(getClusterLabel(1)).toBe('Cluster');
    });

    it('should return plural for count greater than 1', () => {
      expect(getClusterLabel(2)).toBe('Clusters');
      expect(getClusterLabel(10)).toBe('Clusters');
      expect(getClusterLabel(100)).toBe('Clusters');
    });

    it('should return singular for count of 0', () => {
      expect(getClusterLabel(0)).toBe('Cluster');
    });
  });

  describe('getClustersToShow', () => {
    it('should return first 10 items by default', () => {
      const manyClusters = Array(15).fill(null).map((_, i) => ({
        ...mockClusters[0],
        name: `cluster${ i }`,
        clusterId: `id${ i }`,
      }));

      const result = getClustersToShow(manyClusters);

      expect(result).toHaveLength(10);
      expect(result[0].name).toBe('cluster0');
      expect(result[9].name).toBe('cluster9');
    });

    it('should respect custom maxItems parameter', () => {
      const result = getClustersToShow(mockClusters, 2);

      expect(result).toHaveLength(2);
      expect(result[0].name).toBe('cluster1');
      expect(result[1].name).toBe('cluster2');
    });

    it('should return all items if fewer than maxItems', () => {
      const result = getClustersToShow(mockClusters, 10);

      expect(result).toHaveLength(3);
      expect(result).toEqual(mockClusters);
    });

    it('should handle empty array', () => {
      const result = getClustersToShow([]);

      expect(result).toHaveLength(0);
    });
  });

  describe('getGridRows', () => {
    it('should return the item count if less than maxRows', () => {
      expect(getGridRows(3)).toBe(3);
      expect(getGridRows(1)).toBe(1);
    });

    it('should return maxRows if item count exceeds it', () => {
      expect(getGridRows(10)).toBe(5);
      expect(getGridRows(20)).toBe(5);
    });

    it('should respect custom maxRows parameter', () => {
      expect(getGridRows(10, 3)).toBe(3);
      expect(getGridRows(2, 3)).toBe(2);
    });

    it('should handle edge case of 0 items', () => {
      expect(getGridRows(0)).toBe(0);
    });
  });

  describe('hasMoreClusters', () => {
    it('should return true when total count exceeds display count', () => {
      expect(hasMoreClusters(15, 10)).toBe(true);
      expect(hasMoreClusters(11, 10)).toBe(true);
    });

    it('should return false when total count equals display count', () => {
      expect(hasMoreClusters(10, 10)).toBe(false);
    });

    it('should return false when total count is less than display count', () => {
      expect(hasMoreClusters(5, 10)).toBe(false);
    });

    it('should handle edge cases', () => {
      expect(hasMoreClusters(0, 0)).toBe(false);
      expect(hasMoreClusters(1, 0)).toBe(true);
    });
  });
});
