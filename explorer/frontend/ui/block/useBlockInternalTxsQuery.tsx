import { INTERNAL_TX } from 'stubs/internalTx';
import { generateListStub } from 'stubs/utils';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

import type { BlockQuery } from './useBlockQuery';

interface Params {
  heightOrHash: string;
  blockQuery: BlockQuery;
  tab: string;
}

export default function useBlockInternalTxsQuery({ heightOrHash, blockQuery, tab }: Params) {
  const apiQuery = useQueryWithPages({
    resourceName: 'general:block_internal_txs',
    pathParams: { height_or_hash: heightOrHash },
    options: {
      enabled: Boolean(tab === 'internal_txs' && !blockQuery.isPlaceholderData),
      placeholderData: generateListStub<'general:block_internal_txs'>(INTERNAL_TX, 10, { next_page_params: null }),
      refetchOnMount: false,
    },
  });

  return apiQuery;
}
