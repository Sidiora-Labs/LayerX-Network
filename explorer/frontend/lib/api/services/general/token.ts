import type { ApiResource } from '../../types';
import type {
  TokenCounters,
  TokenInfo,
  TokenHolders,
  TokenInventoryResponse,
  TokenInstance,
  TokenInstanceTransfersCount,
  TokenInventoryFilters,
} from 'types/api/token';
import type { TokensResponse, TokensFilters, TokensSorting, TokenInstanceTransferResponse, TokensBridgedFilters } from 'types/api/tokens';
import type { TokenTransferResponse, TokenTransferFilters } from 'types/api/tokenTransfer';

export const GENERAL_API_TOKEN_RESOURCES = {
  // TOKEN
  token: {
    path: '/api/v2/tokens/:hash',
    pathParams: [ 'hash' as const ],
  },
  token_counters: {
    path: '/api/v2/tokens/:hash/counters',
    pathParams: [ 'hash' as const ],
  },
  token_holders: {
    path: '/api/v2/tokens/:hash/holders',
    pathParams: [ 'hash' as const ],
    filterFields: [],
    paginated: true,
  },
  token_transfers: {
    path: '/api/v2/tokens/:hash/transfers',
    pathParams: [ 'hash' as const ],
    filterFields: [],
    paginated: true,
  },
  token_inventory: {
    path: '/api/v2/tokens/:hash/instances',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'holder_address_hash' as const ],
    paginated: true,
  },
  tokens: {
    path: '/api/v2/tokens',
    filterFields: [ 'q' as const, 'type' as const ],
    paginated: true,
  },
  tokens_bridged: {
    path: '/api/v2/tokens/bridged',
    filterFields: [ 'q' as const, 'chain_ids' as const ],
    paginated: true,
  },
  token_csv_export_holders: {
    path: '/api/v2/tokens/:hash/holders/csv',
    pathParams: [ 'hash' as const ],
  },

  // TOKEN INSTANCE
  token_instance: {
    path: '/api/v2/tokens/:hash/instances/:id',
    pathParams: [ 'hash' as const, 'id' as const ],
  },
  token_instance_transfers_count: {
    path: '/api/v2/tokens/:hash/instances/:id/transfers-count',
    pathParams: [ 'hash' as const, 'id' as const ],
  },
  token_instance_transfers: {
    path: '/api/v2/tokens/:hash/instances/:id/transfers',
    pathParams: [ 'hash' as const, 'id' as const ],
    filterFields: [],
    paginated: true,
  },
  token_instance_holders: {
    path: '/api/v2/tokens/:hash/instances/:id/holders',
    pathParams: [ 'hash' as const, 'id' as const ],
    filterFields: [],
    paginated: true,
  },
  token_instance_refresh_metadata: {
    path: '/api/v2/tokens/:hash/instances/:id/refetch-metadata',
    pathParams: [ 'hash' as const, 'id' as const ],
    filterFields: [],
  },

  // TOKEN TRANSFERS
  token_transfers_all: {
    path: '/api/v2/token-transfers',
    filterFields: [ 'type' as const ],
    paginated: true,
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiTokenResourceName = `general:${ keyof typeof GENERAL_API_TOKEN_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiTokenResourcePayload<R extends GeneralApiTokenResourceName> =
R extends 'general:token' ? TokenInfo :
R extends 'general:token_counters' ? TokenCounters :
R extends 'general:token_transfers' ? TokenTransferResponse :
R extends 'general:token_holders' ? TokenHolders :
R extends 'general:token_instance' ? TokenInstance :
R extends 'general:token_instance_transfers_count' ? TokenInstanceTransfersCount :
R extends 'general:token_instance_transfers' ? TokenInstanceTransferResponse :
R extends 'general:token_instance_holders' ? TokenHolders :
R extends 'general:token_inventory' ? TokenInventoryResponse :
R extends 'general:tokens' ? TokensResponse :
R extends 'general:tokens_bridged' ? TokensResponse :
R extends 'general:token_transfers_all' ? TokenTransferResponse :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiTokenPaginationFilters<R extends GeneralApiTokenResourceName> =
R extends 'general:token_transfers' ? TokenTransferFilters :
R extends 'general:token_inventory' ? TokenInventoryFilters :
R extends 'general:tokens' ? TokensFilters :
R extends 'general:tokens_bridged' ? TokensBridgedFilters :
R extends 'general:token_transfers_all' ? TokenTransferFilters :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiTokenPaginationSorting<R extends GeneralApiTokenResourceName> =
R extends 'general:tokens' ? TokensSorting :
R extends 'general:tokens_bridged' ? TokensSorting :
never;
/* eslint-enable @stylistic/indent */
