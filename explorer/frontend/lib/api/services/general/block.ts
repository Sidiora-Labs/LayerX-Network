import type { ApiResource } from '../../types';
import type {
  BlocksResponse,
  BlockTransactionsResponse,
  Block,
  BlockFilters,
  BlockWithdrawalsResponse,
  BlockCountdownResponse,
  BlockInternalTransactionsResponse,
} from 'types/api/block';
import type { DepositsResponse } from 'types/api/deposits';
import type { TTxsWithBlobsFilters } from 'types/api/txsFilters';

export const GENERAL_API_BLOCK_RESOURCES = {
  blocks: {
    path: '/api/v2/blocks',
    filterFields: [ 'type' as const ],
    paginated: true,
  },
  block: {
    path: '/api/v2/blocks/:height_or_hash',
    pathParams: [ 'height_or_hash' as const ],
  },
  block_txs: {
    path: '/api/v2/blocks/:height_or_hash/transactions',
    pathParams: [ 'height_or_hash' as const ],
    filterFields: [ 'type' as const ],
    paginated: true,
  },
  block_internal_txs: {
    path: '/api/v2/blocks/:height_or_hash/internal-transactions',
    pathParams: [ 'height_or_hash' as const ],
    paginated: true,
  },
  block_deposits: {
    path: '/api/v2/blocks/:height_or_hash/beacon/deposits',
    pathParams: [ 'height_or_hash' as const ],
    filterFields: [],
    paginated: true,
  },
  block_withdrawals: {
    path: '/api/v2/blocks/:height_or_hash/withdrawals',
    pathParams: [ 'height_or_hash' as const ],
    filterFields: [],
    paginated: true,
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiBlockResourceName = `general:${ keyof typeof GENERAL_API_BLOCK_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiBlockResourcePayload<R extends GeneralApiBlockResourceName> =
R extends 'general:blocks' ? BlocksResponse :
R extends 'general:block' ? Block :
R extends 'general:block_countdown' ? BlockCountdownResponse :
R extends 'general:block_txs' ? BlockTransactionsResponse :
R extends 'general:block_internal_txs' ? BlockInternalTransactionsResponse :
R extends 'general:block_withdrawals' ? BlockWithdrawalsResponse :
R extends 'general:block_deposits' ? DepositsResponse :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiBlockPaginationFilters<R extends GeneralApiBlockResourceName> =
R extends 'general:blocks' ? BlockFilters :
R extends 'general:block_txs' ? TTxsWithBlobsFilters :
never;
/* eslint-enable @stylistic/indent */
