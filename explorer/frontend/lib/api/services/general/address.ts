import type { ApiResource } from '../../types';
import type {
  AddressCounters,
  AddressBlocksValidatedResponse,
  AddressTokensResponse,
  AddressCollectionsResponse,
  AddressEpochRewardsResponse,
  AddressNFTsResponse,
  AddressWithdrawalsResponse,
  AddressXStarResponse,
  AddressCoinBalanceHistoryChart,
  AddressCoinBalanceHistoryResponse,
  AddressTokenTransferResponse,
  AddressInternalTxsResponse,
  AddressTransactionsResponse,
  AddressTabsCounters,
  Address,
  AddressTxsFilters,
  AddressTokenTransferFilters,
  AddressTokensFilter,
  AddressNFTTokensFilter,
  AddressTokenBalancesResponse,
} from 'types/api/address';
import type { AddressesMetadataSearchFilters, AddressesMetadataSearchResult, AddressesResponse } from 'types/api/addresses';
import type { DepositsResponse } from 'types/api/deposits';
import type { LogsResponseAddress } from 'types/api/log';
import type { TransactionsSorting } from 'types/api/transaction';

export const GENERAL_API_ADDRESS_RESOURCES = {
  // ADDRESSES
  addresses: {
    path: '/api/v2/addresses/',
    filterFields: [ ],
    paginated: true,
  },
  addresses_metadata_search: {
    path: '/api/v2/proxy/metadata/addresses',
    filterFields: [ 'slug' as const, 'tag_type' as const ],
    paginated: true,
  },

  // ADDRESS INFO
  address: {
    path: '/api/v2/addresses/:hash',
    pathParams: [ 'hash' as const ],
  },
  address_counters: {
    path: '/api/v2/addresses/:hash/counters',
    pathParams: [ 'hash' as const ],
  },
  address_tabs_counters: {
    path: '/api/v2/addresses/:hash/tabs-counters',
    pathParams: [ 'hash' as const ],
  },
  address_txs: {
    path: '/api/v2/addresses/:hash/transactions',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'filter' as const ],
    paginated: true,
  },
  address_internal_txs: {
    path: '/api/v2/addresses/:hash/internal-transactions',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'filter' as const ],
    paginated: true,
  },
  address_token_transfers: {
    path: '/api/v2/addresses/:hash/token-transfers',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'filter' as const, 'type' as const, 'token' as const ],
    paginated: true,
  },
  address_blocks_validated: {
    path: '/api/v2/addresses/:hash/blocks-validated',
    pathParams: [ 'hash' as const ],
    filterFields: [ ],
    paginated: true,
  },
  address_coin_balance: {
    path: '/api/v2/addresses/:hash/coin-balance-history',
    pathParams: [ 'hash' as const ],
    filterFields: [ ],
    paginated: true,
  },
  address_coin_balance_chart: {
    path: '/api/v2/addresses/:hash/coin-balance-history-by-day',
    pathParams: [ 'hash' as const ],
    filterFields: [ ],
  },
  address_logs: {
    path: '/api/v2/addresses/:hash/logs',
    pathParams: [ 'hash' as const ],
    filterFields: [ ],
    paginated: true,
  },
  address_tokens: {
    path: '/api/v2/addresses/:hash/tokens',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'type' as const ],
    paginated: true,
  },
  address_token_balances: {
    path: '/api/v2/addresses/:hash/token-balances',
    pathParams: [ 'hash' as const ],
    filterFields: [ ],
  },
  address_nfts: {
    path: '/api/v2/addresses/:hash/nft',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'type' as const ],
    paginated: true,
  },
  address_collections: {
    path: '/api/v2/addresses/:hash/nft/collections',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'type' as const ],
    paginated: true,
  },
  address_deposits: {
    path: '/api/v2/addresses/:hash/beacon/deposits',
    pathParams: [ 'hash' as const ],
    filterFields: [],
    paginated: true,
  },
  address_withdrawals: {
    path: '/api/v2/addresses/:hash/withdrawals',
    pathParams: [ 'hash' as const ],
    filterFields: [],
    paginated: true,
  },
  address_epoch_rewards: {
    path: '/api/v2/addresses/:hash/celo/election-rewards',
    pathParams: [ 'hash' as const ],
    filterFields: [],
    paginated: true,
  },
  address_xstar_score: {
    path: '/api/v2/proxy/3rdparty/xname/addresses/:hash',
    pathParams: [ 'hash' as const ],
  },
  address_3rd_party_info: {
    path: '/api/v2/proxy/3rdparty/:name',
    pathParams: [ 'name' as const ],
    filterFields: [ 'address' as const, 'chain_id' as const ],
  },

  // CSV EXPORTS
  address_csv_export_txs: {
    path: '/api/v2/addresses/:hash/transactions/csv',
    pathParams: [ 'hash' as const ],
  },
  address_csv_export_internal_txs: {
    path: '/api/v2/addresses/:hash/internal-transactions/csv',
    pathParams: [ 'hash' as const ],
  },
  address_csv_export_token_transfers: {
    path: '/api/v2/addresses/:hash/token-transfers/csv',
    pathParams: [ 'hash' as const ],
  },
  address_csv_export_logs: {
    path: '/api/v2/addresses/:hash/logs/csv',
    pathParams: [ 'hash' as const ],
  },
  address_csv_export_celo_election_rewards: {
    path: '/api/v2/addresses/:hash/celo/election-rewards/csv',
    pathParams: [ 'hash' as const ],
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiAddressResourceName = `general:${ keyof typeof GENERAL_API_ADDRESS_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiAddressResourcePayload<R extends GeneralApiAddressResourceName> =
R extends 'general:addresses' ? AddressesResponse :
R extends 'general:addresses_metadata_search' ? AddressesMetadataSearchResult :
R extends 'general:address' ? Address :
R extends 'general:address_counters' ? AddressCounters :
R extends 'general:address_tabs_counters' ? AddressTabsCounters :
R extends 'general:address_txs' ? AddressTransactionsResponse :
R extends 'general:address_internal_txs' ? AddressInternalTxsResponse :
R extends 'general:address_token_transfers' ? AddressTokenTransferResponse :
R extends 'general:address_blocks_validated' ? AddressBlocksValidatedResponse :
R extends 'general:address_coin_balance' ? AddressCoinBalanceHistoryResponse :
R extends 'general:address_coin_balance_chart' ? AddressCoinBalanceHistoryChart :
R extends 'general:address_logs' ? LogsResponseAddress :
R extends 'general:address_tokens' ? AddressTokensResponse :
R extends 'general:address_token_balances' ? AddressTokenBalancesResponse :
R extends 'general:address_nfts' ? AddressNFTsResponse :
R extends 'general:address_collections' ? AddressCollectionsResponse :
R extends 'general:address_withdrawals' ? AddressWithdrawalsResponse :
R extends 'general:address_deposits' ? DepositsResponse :
R extends 'general:address_epoch_rewards' ? AddressEpochRewardsResponse :
R extends 'general:address_xstar_score' ? AddressXStarResponse :
R extends 'general:address_3rd_party_info' ? unknown :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiAddressPaginationFilters<R extends GeneralApiAddressResourceName> =
R extends 'general:addresses_metadata_search' ? AddressesMetadataSearchFilters :
R extends 'general:address_txs' | 'general:address_internal_txs' ? AddressTxsFilters :
R extends 'general:address_token_transfers' ? AddressTokenTransferFilters :
R extends 'general:address_tokens' ? AddressTokensFilter :
R extends 'general:address_nfts' ? AddressNFTTokensFilter :
R extends 'general:address_collections' ? AddressNFTTokensFilter :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiAddressPaginationSorting<R extends GeneralApiAddressResourceName> =
R extends 'general:address_txs' ? TransactionsSorting :
never;
/* eslint-enable @stylistic/indent */
