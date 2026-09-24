import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';

const feature = config.features.nameServices;

export function useAddressClusters(addressHash: string, isEnabled: boolean = true) {
  return useApiQuery('clusters:get_clusters_by_address', {
    queryParams: {
      input: JSON.stringify({
        address: addressHash,
      }),
    },
    queryOptions: {
      enabled: Boolean(addressHash) && feature.isEnabled && feature.clusters.isEnabled && isEnabled,
    },
  });
}
