import type { ApiResource } from '../types';
import type {
  ClustersByAddressResponse,
  ClusterByNameResponse,
  ClustersLeaderboardResponse,
  ClustersDirectoryResponse,
  ClustersByAddressQueryParams,
  ClusterByNameQueryParams,
  ClustersLeaderboardQueryParams,
  ClustersDirectoryQueryParams,
  ClusterByIdQueryParams,
  ClusterByIdResponse,
} from 'types/api/clusters';

export const CLUSTERS_API_RESOURCES = {
  get_clusters_by_address: {
    path: '/v1/trpc/names.getNamesByOwnerAddress',
    pathParams: [],
  },
  get_cluster_by_name: {
    path: '/v1/trpc/names.get',
    pathParams: [],
  },
  get_cluster_by_id: {
    path: '/v1/trpc/clusters.getClusterById',
    pathParams: [],
  },
  get_leaderboard: {
    path: '/v1/trpc/names.leaderboard',
    pathParams: [],
  },
  get_directory: {
    path: '/v1/trpc/names.search',
    pathParams: [],
  },
} satisfies Record<string, ApiResource>;

export type ClustersApiResourceName = `clusters:${ keyof typeof CLUSTERS_API_RESOURCES }`;

export type ClustersApiResourcePayload<R extends ClustersApiResourceName> =
  R extends 'clusters:get_clusters_by_address' ? ClustersByAddressResponse :
    R extends 'clusters:get_cluster_by_name' ? ClusterByNameResponse :
      R extends 'clusters:get_cluster_by_id' ? ClusterByIdResponse :
        R extends 'clusters:get_leaderboard' ? ClustersLeaderboardResponse :
          R extends 'clusters:get_directory' ? ClustersDirectoryResponse :
            never;

export type ClustersApiQueryParams<R extends ClustersApiResourceName> =
  R extends 'clusters:get_clusters_by_address' ? ClustersByAddressQueryParams :
    R extends 'clusters:get_cluster_by_name' ? ClusterByNameQueryParams :
      R extends 'clusters:get_cluster_by_id' ? ClusterByIdQueryParams :
        R extends 'clusters:get_leaderboard' ? ClustersLeaderboardQueryParams :
          R extends 'clusters:get_directory' ? ClustersDirectoryQueryParams :
            never;

export type ClustersApiPaginationFilters = never;
export type ClustersApiPaginationSorting = never;
