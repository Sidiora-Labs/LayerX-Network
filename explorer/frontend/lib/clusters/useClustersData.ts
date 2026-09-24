import React from 'react';

import type { ClustersByAddressObject } from 'types/api/clusters';
import { ClustersOrderBy } from 'types/api/clusters';

import useApiQuery from 'lib/api/useApiQuery';
import { detectInputType } from 'lib/clusters/detectInputType';
import { CLUSTER_ITEM } from 'stubs/clusters';

export function useClustersData(debouncedSearchTerm: string, viewMode: string, page: number) {
  const ITEMS_PER_PAGE = 50;

  const inputType = React.useMemo(() => {
    if (!debouncedSearchTerm) return 'cluster_name';
    return detectInputType(debouncedSearchTerm);
  }, [ debouncedSearchTerm ]);

  const showDirectoryView = viewMode === 'directory' || Boolean(debouncedSearchTerm);

  const leaderboardQuery = useApiQuery('clusters:get_leaderboard', {
    queryParams: {
      input: JSON.stringify({
        offset: (page - 1) * ITEMS_PER_PAGE,
        limit: ITEMS_PER_PAGE,
        orderBy: ClustersOrderBy.RANK_ASC,
      }),
    },
    queryOptions: {
      enabled: !showDirectoryView,
      placeholderData: (previousData) => {
        if (previousData) return previousData;
        return {
          result: {
            data: Array(ITEMS_PER_PAGE).fill(CLUSTER_ITEM),
          },
        };
      },
    },
  });

  const getDirectoryOrderBy = React.useMemo(() => {
    if (debouncedSearchTerm) {
      return ClustersOrderBy.NAME_ASC;
    }
    return ClustersOrderBy.CREATED_AT_DESC;
  }, [ debouncedSearchTerm ]);

  const directoryQuery = useApiQuery('clusters:get_directory', {
    queryParams: {
      input: JSON.stringify({
        offset: (page - 1) * ITEMS_PER_PAGE,
        limit: ITEMS_PER_PAGE,
        orderBy: getDirectoryOrderBy,
        query: debouncedSearchTerm || '',
      }),
    },
    queryOptions: {
      enabled: showDirectoryView && inputType === 'cluster_name',
      placeholderData: (previousData) => {
        if (previousData) return previousData;
        return {
          result: {
            data: {
              total: 1000,
              items: Array(ITEMS_PER_PAGE).fill(CLUSTER_ITEM),
            },
          },
        };
      },
    },
  });

  const addressQuery = useApiQuery('clusters:get_clusters_by_address', {
    queryParams: {
      input: JSON.stringify({
        address: debouncedSearchTerm,
      }),
    },
    queryOptions: {
      enabled: showDirectoryView && inputType === 'address',
      placeholderData: (previousData) => {
        if (previousData) return previousData;
        return {
          result: {
            data: Array(ITEMS_PER_PAGE).fill(CLUSTER_ITEM),
          },
        };
      },
    },
  });

  const clusterDetailsQuery = useApiQuery('clusters:get_cluster_by_id', {
    queryParams: {
      input: JSON.stringify({
        id: addressQuery.data?.result?.data?.[0]?.clusterId || '',
      }),
    },
    queryOptions: {
      enabled: (
        showDirectoryView &&
        inputType === 'address' &&
        Boolean((addressQuery.data?.result?.data?.[0] as ClustersByAddressObject & { clusterId?: string })?.clusterId)
      ),
    },
  });

  const { data, isError, isPlaceholderData: isLoading } = (() => {
    if (!showDirectoryView) return leaderboardQuery;
    if (inputType === 'address') return addressQuery;
    return directoryQuery;
  })();

  return {
    data,
    clusterDetails: clusterDetailsQuery.data,
    isError,
    isLoading,
    isClusterDetailsLoading: clusterDetailsQuery.isLoading,
  };
}
