import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { PAXEER_X_TX_STATUS } from 'stubs/paxeerXLists';

const feature = config.features.paxeerXLists;

export default function useTxStatusLadderQuery(hash: string | undefined) {
  return useApiQuery('general:paxeer_x_tx_status', {
    pathParams: { hash },
    queryOptions: {
      enabled: feature.isEnabled && Boolean(hash),
      placeholderData: PAXEER_X_TX_STATUS,
      refetchOnMount: false,
    },
  });
}
