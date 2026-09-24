import type { ClustersByAddressResponse } from 'types/api/clusters';

export type ClusterData = ClustersByAddressResponse['result']['data'][0];

export function filterOwnedClusters(clusters: Array<ClusterData>, ownerAddress: string): Array<ClusterData> {
  return clusters.filter((cluster) =>
    cluster.owner && cluster.owner.toLowerCase() === ownerAddress.toLowerCase(),
  );
}

export function getTotalRecordsDisplay(count: number): string {
  return count > 40 ? '40+' : count.toString();
}

export function getClusterLabel(count: number): string {
  return count > 1 ? 'Clusters' : 'Cluster';
}

export function getClustersToShow(clusters: Array<ClusterData>, maxItems: number = 10): Array<ClusterData> {
  return clusters.slice(0, maxItems);
}

export function getGridRows(itemCount: number, maxRows: number = 5): number {
  return Math.min(itemCount, maxRows);
}

export function hasMoreClusters(totalCount: number, displayCount: number): boolean {
  return totalCount > displayCount;
}
