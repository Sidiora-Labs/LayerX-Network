import { ClustersOrderBy } from 'types/api/clusters';
import type { ClustersLeaderboardObject, ClustersDirectoryObject, ClustersByAddressObject } from 'types/api/clusters';

export type ViewMode = 'leaderboard' | 'directory';
export type InputType = 'address' | 'cluster_name';

export function getViewModeOrderBy(viewMode: ViewMode, hasSearchTerm: boolean): ClustersOrderBy {
  if (viewMode === 'leaderboard') return ClustersOrderBy.RANK_ASC;
  if (hasSearchTerm) return ClustersOrderBy.NAME_ASC;
  return ClustersOrderBy.CREATED_AT_DESC;
}

export function shouldShowDirectoryView(viewMode: ViewMode, hasSearchTerm: boolean): boolean {
  return viewMode === 'directory' || hasSearchTerm;
}

export function transformLeaderboardData(
  data: unknown,
  showDirectoryView: boolean,
): Array<ClustersLeaderboardObject> {
  if (!data || showDirectoryView) return [];

  if (data && typeof data === 'object' && 'result' in data) {
    const result = (data as Record<string, unknown>).result;
    if (result && typeof result === 'object' && 'data' in result && Array.isArray(result.data)) {
      return result.data as Array<ClustersLeaderboardObject>;
    }
  }

  return [];
}

export function transformAddressDataToDirectory(
  addressData: Array<ClustersByAddressObject>,
  clusterDetails: unknown,
): Array<ClustersDirectoryObject> {
  const clusterDetailsData = clusterDetails &&
    typeof clusterDetails === 'object' &&
    'result' in clusterDetails &&
    clusterDetails.result &&
    typeof clusterDetails.result === 'object' &&
    'data' in clusterDetails.result ? clusterDetails.result.data : null;

  const allChainIds = clusterDetailsData &&
    typeof clusterDetailsData === 'object' &&
    'wallets' in clusterDetailsData &&
    Array.isArray(clusterDetailsData.wallets) ?
    clusterDetailsData.wallets.flatMap(
      (wallet: unknown) => {
        if (wallet && typeof wallet === 'object' && 'chainIds' in wallet && Array.isArray(wallet.chainIds)) {
          return wallet.chainIds as Array<string>;
        }
        return [];
      },
    ) : [];
  const uniqueChainIds = [ ...new Set(allChainIds) ] as Array<string>;

  return addressData.map((item) => ({
    name: item.name,
    isTestnet: item.isTestnet,
    createdAt: item.createdAt,
    owner: item.owner,
    totalWeiAmount: item.totalWeiAmount,
    updatedAt: item.updatedAt,
    updatedBy: item.updatedBy,
    chainIds: uniqueChainIds,
  }));
}

export function transformFullDirectoryData(
  data: unknown,
  clusterDetails: unknown,
  inputType: InputType,
  showDirectoryView: boolean,
): Array<ClustersDirectoryObject> {
  if (!showDirectoryView || !data) return [];

  if (inputType === 'address') {
    const addressData = data &&
      typeof data === 'object' &&
      'result' in data &&
      data.result &&
      typeof data.result === 'object' &&
      'data' in data.result ? data.result.data as Array<ClustersByAddressObject> : null;
    if (addressData && Array.isArray(addressData)) {
      return transformAddressDataToDirectory(addressData, clusterDetails);
    }
  } else {
    const apiData = data &&
      typeof data === 'object' &&
      'result' in data &&
      data.result &&
      typeof data.result === 'object' &&
      'data' in data.result ? data.result.data : null;
    if (apiData && typeof apiData === 'object' && 'items' in apiData && Array.isArray(apiData.items)) {
      return apiData.items as Array<ClustersDirectoryObject>;
    }
  }

  return [];
}

export function applyDirectoryPagination(
  fullDirectoryData: Array<ClustersDirectoryObject>,
  inputType: InputType,
  page: number,
  limit = 50,
): Array<ClustersDirectoryObject> {
  if (inputType === 'address') {
    const startIndex = (page - 1) * limit;
    const endIndex = startIndex + limit;
    return fullDirectoryData.slice(startIndex, endIndex);
  }
  return fullDirectoryData;
}

export function calculateHasNextPage(
  data: unknown,
  leaderboardDataLength: number,
  fullDirectoryDataLength: number,
  showDirectoryView: boolean,
  inputType: InputType,
  page: number,
  hasSearchTerm: boolean,
  limit = 50,
): boolean {
  if (showDirectoryView) {
    if (inputType === 'address') {
      return page * limit < fullDirectoryDataLength;
    } else {
      if (hasSearchTerm) return false;
      const apiData = data &&
        typeof data === 'object' &&
        'result' in data &&
        data.result &&
        typeof data.result === 'object' &&
        'data' in data.result ? data.result.data : null;
      if (apiData && typeof apiData === 'object' && 'total' in apiData && typeof apiData.total === 'number') {
        return (page * limit) < apiData.total;
      }
      return false;
    }
  }
  return leaderboardDataLength === limit;
}

export function isValidViewMode(value: string): value is ViewMode {
  return value === 'leaderboard' || value === 'directory';
}

export function getDefaultViewMode(): ViewMode {
  return 'directory';
}

export function getCurrentDataLength(
  showDirectoryView: boolean,
  directoryDataLength: number,
  leaderboardDataLength: number,
): number {
  return showDirectoryView ? directoryDataLength : leaderboardDataLength;
}
