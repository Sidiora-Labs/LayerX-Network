import { useQuery } from '@tanstack/react-query';
import React from 'react';

import type { SearchResultCluster } from 'types/api/search';

import config from 'configs/app';
import type { ResourcePayload, ResourceError } from 'lib/api/resources';
import useApiFetch from 'lib/api/useApiFetch';
import { getResourceKey } from 'lib/api/useApiQuery';

import useQuickSearchQuery from './useQuickSearchQuery';

const nameServicesFeature = config.features.nameServices;
const isClustersEnabled = nameServicesFeature.isEnabled && nameServicesFeature.clusters.isEnabled;

function isClusterSearch(term: string): boolean {
  const trimmed = term.trim();
  const hasTrailingSlash = trimmed.endsWith('/');
  const looksLikeCluster = trimmed.includes('/') || hasTrailingSlash;

  return looksLikeCluster;
}

function extractClusterName(term: string): string {
  const trimmed = term.trim();
  return trimmed.endsWith('/') ? trimmed.slice(0, -1) : trimmed;
}

function transformClusterToSearchResult(cluster: {
  name: string;
  clusterId?: string;
  owner: string;
  createdAt?: string;
  expiresAt?: string | null;
  backingWei?: string;
  isTestnet?: boolean;
}, ownerAddress: string): SearchResultCluster {
  return {
    type: 'cluster',
    name: cluster.name,
    address_hash: ownerAddress,
    is_smart_contract_verified: false,
    is_smart_contract_address: false,
    cluster_info: {
      cluster_id: cluster.clusterId || cluster.name,
      name: cluster.name,
      owner: cluster.owner,
      created_at: cluster.createdAt,
      expires_at: cluster.expiresAt,
      total_wei_amount: cluster.backingWei,
      is_testnet: cluster.isTestnet,
    },
  };
}

export default function useSearchWithClusters() {
  const quickSearch = useQuickSearchQuery();

  const isClusterQuery = isClustersEnabled ?
    isClusterSearch(quickSearch.debouncedSearchTerm) : false;

  const clusterName = isClusterQuery ? extractClusterName(quickSearch.debouncedSearchTerm) : '';

  const RESOURCE_NAME = 'clusters:get_cluster_by_name';
  type ClusterQueryResult = ResourcePayload<typeof RESOURCE_NAME>;

  const apiFetch = useApiFetch();

  const clusterQuery = useQuery<ClusterQueryResult | null, ResourceError<unknown>, Array<SearchResultCluster>>({
    queryKey: getResourceKey(RESOURCE_NAME, { queryParams: { input: clusterName } }),
    queryFn: async({ signal }) => {
      try {
        const result = await apiFetch(RESOURCE_NAME, {
          queryParams: { input: JSON.stringify({ name: clusterName }) },
          fetchParams: { signal },
        }) as ClusterQueryResult;
        return result;
      } catch (error) {
        return null;
      }
    },
    enabled: isClustersEnabled && isClusterQuery && clusterName.length > 0,
    select: (data) => {
      if (!data?.result?.data) return [];
      return [ transformClusterToSearchResult(data.result.data, data.result.data.owner) ];
    },
  });

  const combinedQuery = React.useMemo(() => {
    if (!isClustersEnabled || !isClusterQuery) {
      return quickSearch.query;
    }

    return clusterQuery;
  }, [ isClusterQuery, quickSearch, clusterQuery ]);

  const result = React.useMemo(() => ({
    ...quickSearch,
    query: combinedQuery,
  }), [
    quickSearch,
    combinedQuery,
  ]);

  return isClustersEnabled ? result : quickSearch;
}
