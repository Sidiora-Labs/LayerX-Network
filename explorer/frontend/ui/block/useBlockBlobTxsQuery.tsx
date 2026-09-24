import { TX } from 'stubs/tx';
import { generateListStub } from 'stubs/utils';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

import type { BlockQuery } from './useBlockQuery';

interface Params {
  heightOrHash: string;
  blockQuery: BlockQuery;
  tab: string;
}

export default function useBlockBlobTxsQuery({ heightOrHash, blockQuery, tab }: Params) {
  const apiQuery = useQueryWithPages({
    resourceName: 'general:block_txs',
    pathParams: { height_or_hash: heightOrHash },
    filters: { type: 'blob_transaction' },
    options: {
      enabled: Boolean(tab === 'blob_txs' && !blockQuery.isPlaceholderData && blockQuery.data?.blob_transactions_count),
      placeholderData: generateListStub<'general:block_txs'>(TX, 3, { next_page_params: null }),
      refetchOnMount: false,
    },
  });

  return apiQuery;
}
