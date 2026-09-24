import config from 'configs/app';
import { DEPOSIT } from 'stubs/deposits';
import { generateListStub } from 'stubs/utils';
import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

import type { BlockQuery } from './useBlockQuery';

export type BlockDepositsQuery = QueryWithPagesResult<'general:block_deposits'> & {
  isDegradedData: boolean;
};

interface Params {
  heightOrHash: string;
  blockQuery: BlockQuery;
  tab: string;
}

const beaconChainFeature = config.features.beaconChain;

// No deposits data in RPC, so we use API only
export default function useBlockDepositsQuery({ heightOrHash, blockQuery, tab }: Params): BlockDepositsQuery {
  const apiQuery = useQueryWithPages({
    resourceName: 'general:block_deposits',
    pathParams: { height_or_hash: heightOrHash },
    options: {
      enabled:
        tab === 'deposits' &&
        beaconChainFeature.isEnabled && !beaconChainFeature.withdrawalsOnly &&
        !blockQuery.isPlaceholderData && !blockQuery.isDegradedData,
      placeholderData: generateListStub<'general:block_deposits'>(DEPOSIT, 50, { next_page_params: {
        index: 5,
        items_count: 50,
      } }),
      refetchOnMount: false,
    },
  });

  return {
    ...apiQuery,
    isDegradedData: false,
  };
}
