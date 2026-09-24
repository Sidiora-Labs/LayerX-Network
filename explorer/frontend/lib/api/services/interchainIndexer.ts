import type { ApiResource } from '../types';
import type * as interchainIndexer from '@blockscout/interchain-indexer-types';
import type { CrossChainMessageFilters, CrossChainTransferFilters } from 'types/api/interchainIndexer';

export const INTERCHAIN_INDEXER_API_RESOURCES = {
  messages: {
    path: '/api/v1/interchain/messages',
    filterFields: [ 'q' as const ],
    paginated: true,
  },
  message: {
    path: '/api/v1/interchain/messages/:id',
    pathParams: [ 'id' as const ],
  },
  tx_messages: {
    path: '/api/v1/interchain/messages\\:byTx/:hash',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'q' as const ],
    paginated: true,
  },
  address_messages: {
    path: '/api/v1/interchain/messages\\:byAddress/:hash',
    pathParams: [ 'hash' as const ],
    paginated: true,
  },
  transfers: {
    path: '/api/v1/interchain/transfers',
    filterFields: [ 'q' as const ],
    paginated: true,
  },
  tx_transfers: {
    path: '/api/v1/interchain/transfers\\:byTx/:hash',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'q' as const ],
    paginated: true,
  },
  address_transfers: {
    path: '/api/v1/interchain/transfers\\:byAddress/:hash',
    pathParams: [ 'hash' as const ],
    paginated: true,
  },
  stats_daily: {
    path: '/api/v1/stats/daily',
  },
  stats_common: {
    path: '/api/v1/stats/common',
  },
} satisfies Record<string, ApiResource>;

export type InterchainIndexerApiResourceName = `interchainIndexer:${ keyof typeof INTERCHAIN_INDEXER_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type InterchainIndexerApiResourcePayload<R extends InterchainIndexerApiResourceName> =
R extends 'interchainIndexer:messages' ? interchainIndexer.GetMessagesResponse :
R extends 'interchainIndexer:message' ? interchainIndexer.InterchainMessage :
R extends 'interchainIndexer:tx_messages' ? interchainIndexer.GetMessagesResponse :
R extends 'interchainIndexer:address_messages' ? interchainIndexer.GetMessagesResponse :
R extends 'interchainIndexer:transfers' ? interchainIndexer.GetTransfersResponse :
R extends 'interchainIndexer:tx_transfers' ? interchainIndexer.GetTransfersResponse :
R extends 'interchainIndexer:address_transfers' ? interchainIndexer.GetTransfersResponse :
R extends 'interchainIndexer:stats_daily' ? interchainIndexer.GetDailyStatisticsResponse :
R extends 'interchainIndexer:stats_common' ? interchainIndexer.GetCommonStatisticsResponse :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type InterchainIndexerApiPaginationFilters<R extends InterchainIndexerApiResourceName> =
R extends 'interchainIndexer:messages' ? CrossChainMessageFilters :
R extends 'interchainIndexer:transfers' ? CrossChainTransferFilters :
never;
/* eslint-enable @stylistic/indent */
