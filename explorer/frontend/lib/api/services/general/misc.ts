import type { ApiResource } from '../../types';
import type { AdvancedFilterParams, AdvancedFilterResponse, AdvancedFilterMethodsResponse } from 'types/api/advancedFilter';
import type {
  ArbitrumL2TxnBatchesItem,
  ArbitrumLatestDepositsResponse,
} from 'types/api/arbitrumL2';
import type { Blob } from 'types/api/blobs';
import type { Block } from 'types/api/block';
import type { ChartMarketResponse, ChartSecondaryCoinPriceResponse, ChartTransactionResponse } from 'types/api/charts';
import type { BackendConfig, BackendVersionConfig, CeloConfig, ContractLanguagesConfig, CsvExportConfig } from 'types/api/configs';
import type { HotContractsFilters, HotContractsResponse, HotContractsSorting } from 'types/api/contracts';
import type { DepositsResponse, DepositsCounters } from 'types/api/deposits';
import type { CeloEpochDetails, CeloEpochElectionRewardDetailsResponse, CeloEpochListResponse } from 'types/api/epochs';
import type { IndexingStatus } from 'types/api/indexingStatus';
import type { NovesAccountHistoryResponse, NovesDescribeTxsResponse, NovesResponseData } from 'types/api/noves';
import type {
  OptimisticL2DepositsItem,
} from 'types/api/optimisticL2';
import type { SearchRedirectResult, SearchResult, SearchResultFilters, SearchResultItem } from 'types/api/search';
import type { HomeStats } from 'types/api/stats';
import type {
  Transaction,
} from 'types/api/transaction';
import type { TxInterpretationResponse } from 'types/api/txInterpretation';
import type { UserOpsResponse, UserOp, UserOpsFilters, UserOpsAccount } from 'types/api/userOps';
import type {
  ValidatorsStabilityCountersResponse,
  ValidatorsStabilityFilters,
  ValidatorsStabilityResponse,
  ValidatorsStabilitySorting,
  ValidatorsBlackfortCountersResponse,
  ValidatorsBlackfortResponse,
  ValidatorsBlackfortSorting,
  ValidatorsZilliqaResponse,
  ValidatorZilliqa,
} from 'types/api/validators';
import type { WithdrawalsResponse, WithdrawalsCounters } from 'types/api/withdrawals';
import type {
  ZkEvmL2TxnBatchesItem,
} from 'types/api/zkEvmL2';

export const GENERAL_API_MISC_RESOURCES = {
  // WITHDRAWALS
  withdrawals: {
    path: '/api/v2/withdrawals',
    filterFields: [],
    paginated: true,
  },
  withdrawals_counters: {
    path: '/api/v2/withdrawals/counters',
  },

  // DEPOSITS
  deposits: {
    path: '/api/v2/beacon/deposits',
    filterFields: [],
    paginated: true,
  },
  deposits_counters: {
    path: '/api/v2/beacon/deposits/count',
  },

  // APP STATS
  stats: {
    path: '/api/v2/stats',
    headers: {
      'updated-gas-oracle': 'true',
    },
  },
  stats_charts_txs: {
    path: '/api/v2/stats/charts/transactions',
  },
  stats_charts_market: {
    path: '/api/v2/stats/charts/market',
  },
  stats_charts_secondary_coin_price: {
    path: '/api/v2/stats/charts/secondary-coin-market',
  },
  stats_hot_contracts: {
    path: '/api/v2/stats/hot-smart-contracts',
    paginated: true,
    filterFields: [ 'scale' as const ],
  },

  // HOMEPAGE
  homepage_blocks: {
    path: '/api/v2/main-page/blocks',
  },
  homepage_optimistic_deposits: {
    path: '/api/v2/main-page/optimism-deposits',
  },
  homepage_arbitrum_deposits: {
    path: '/api/v2/main-page/arbitrum/messages/to-rollup',
  },
  homepage_txs: {
    path: '/api/v2/main-page/transactions',
  },
  homepage_zkevm_l2_batches: {
    path: '/api/v2/main-page/zkevm/batches/confirmed',
  },
  homepage_arbitrum_l2_batches: {
    path: '/api/v2/main-page/arbitrum/batches/committed',
  },
  homepage_txs_watchlist: {
    path: '/api/v2/main-page/transactions/watchlist',
  },
  homepage_indexing_status: {
    path: '/api/v2/main-page/indexing-status',
  },
  homepage_zkevm_latest_batch: {
    path: '/api/v2/main-page/zkevm/batches/latest-number',
  },
  homepage_zksync_latest_batch: {
    path: '/api/v2/main-page/zksync/batches/latest-number',
  },
  homepage_arbitrum_latest_batch: {
    path: '/api/v2/main-page/arbitrum/batches/latest-number',
  },

  // SEARCH
  quick_search: {
    path: '/api/v2/search/quick',
    filterFields: [ 'q' ],
  },
  search: {
    path: '/api/v2/search',
    filterFields: [ 'q' ],
    paginated: true,
  },
  search_check_redirect: {
    path: '/api/v2/search/check-redirect',
  },

  // NOVES-FI
  noves_transaction: {
    path: '/api/v2/proxy/3rdparty/noves-fi/transactions/:hash',
    pathParams: [ 'hash' as const ],
  },
  noves_address_history: {
    path: '/api/v2/proxy/3rdparty/noves-fi/addresses/:address/transactions',
    pathParams: [ 'address' as const ],
    filterFields: [],
    paginated: true,
  },
  noves_describe_txs: {
    path: '/api/v2/proxy/3rdparty/noves-fi/transaction-descriptions',
  },

  // USER OPS
  user_ops: {
    path: '/api/v2/proxy/account-abstraction/operations',
    filterFields: [ 'transaction_hash' as const, 'sender' as const ],
    paginated: true,
  },
  user_op: {
    path: '/api/v2/proxy/account-abstraction/operations/:hash',
    pathParams: [ 'hash' as const ],
  },
  user_ops_account: {
    path: '/api/v2/proxy/account-abstraction/accounts/:hash',
    pathParams: [ 'hash' as const ],
  },
  user_op_interpretation: {
    path: '/api/v2/proxy/account-abstraction/operations/:hash/summary',
    pathParams: [ 'hash' as const ],
  },

  // VALIDATORS
  validators_stability: {
    path: '/api/v2/validators/stability',
    filterFields: [ 'address_hash' as const, 'state_filter' as const ],
    paginated: true,
  },
  validators_stability_counters: {
    path: '/api/v2/validators/stability/counters',
  },
  validators_blackfort: {
    path: '/api/v2/validators/blackfort',
    filterFields: [],
    paginated: true,
  },
  validators_blackfort_counters: {
    path: '/api/v2/validators/blackfort/counters',
  },
  validators_zilliqa: {
    path: '/api/v2/validators/zilliqa',
    filterFields: [],
    paginated: true,
  },
  validator_zilliqa: {
    path: '/api/v2/validators/zilliqa/:bls_public_key',
    pathParams: [ 'bls_public_key' as const ],
    filterFields: [],
  },

  // BLOBS
  blob: {
    path: '/api/v2/blobs/:hash',
    pathParams: [ 'hash' as const ],
  },

  // EPOCHS
  epochs_celo: {
    path: '/api/v2/celo/epochs',
    filterFields: [],
    paginated: true,
  },
  epoch_celo: {
    path: '/api/v2/celo/epochs/:number',
    pathParams: [ 'number' as const ],
  },
  epoch_celo_election_rewards: {
    path: '/api/v2/celo/epochs/:number/election-rewards/:reward_type',
    pathParams: [ 'number' as const, 'reward_type' as const ],
    filterFields: [],
    paginated: true,
  },

  // ADVANCED FILTER
  advanced_filter: {
    path: '/api/v2/advanced-filters',
    filterFields: [
      'transaction_types' as const,
      'methods' as const,
      'methods_names' as const /* frontend only */,
      'age_from' as const,
      'age_to' as const,
      'age' as const /* frontend only */,
      'from_address_hashes_to_include' as const,
      'from_address_hashes_to_exclude' as const,
      'to_address_hashes_to_include' as const,
      'to_address_hashes_to_exclude' as const,
      'address_relation' as const,
      'amount_from' as const,
      'amount_to' as const,
      'token_contract_address_hashes_to_include' as const,
      'token_contract_symbols_to_include' as const /* frontend only */,
      'token_contract_address_hashes_to_exclude' as const,
      'token_contract_symbols_to_exclude' as const /* frontend only */,
      'block_number' as const,
      'transaction_index' as const,
      'internal_transaction_index' as const,
      'token_transfer_index' as const,
    ],
    paginated: true,
  },
  advanced_filter_methods: {
    path: '/api/v2/advanced-filters/methods',
    filterFields: [ 'q' as const ],
  },
  advanced_filter_csv: {
    path: '/api/v2/advanced-filters/csv',
  },

  // CONFIGS
  config_backend: {
    path: '/api/v2/config/backend',
  },
  config_backend_version: {
    path: '/api/v2/config/backend-version',
  },
  config_csv_export: {
    path: '/api/v2/config/csv-export',
  },
  config_celo: {
    path: '/api/v2/config/celo',
  },
  config_contract_languages: {
    path: '/api/v2/config/smart-contracts/languages',
  },

  // OTHER
  api_v2_key: {
    path: '/api/v2/key',
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiMiscResourceName = `general:${ keyof typeof GENERAL_API_MISC_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiMiscResourcePayload<R extends GeneralApiMiscResourceName> =
R extends 'general:stats' ? HomeStats :
R extends 'general:stats_charts_txs' ? ChartTransactionResponse :
R extends 'general:stats_charts_market' ? ChartMarketResponse :
R extends 'general:stats_charts_secondary_coin_price' ? ChartSecondaryCoinPriceResponse :
R extends 'general:stats_hot_contracts' ? HotContractsResponse :
R extends 'general:homepage_blocks' ? Array<Block> :
R extends 'general:homepage_txs' ? Array<Transaction> :
R extends 'general:homepage_txs_watchlist' ? Array<Transaction> :
R extends 'general:homepage_optimistic_deposits' ? Array<OptimisticL2DepositsItem> :
R extends 'general:homepage_arbitrum_deposits' ? ArbitrumLatestDepositsResponse :
R extends 'general:homepage_zkevm_l2_batches' ? { items: Array<ZkEvmL2TxnBatchesItem> } :
R extends 'general:homepage_arbitrum_l2_batches' ? { items: Array<ArbitrumL2TxnBatchesItem> } :
R extends 'general:homepage_indexing_status' ? IndexingStatus :
R extends 'general:homepage_zkevm_latest_batch' ? number :
R extends 'general:homepage_zksync_latest_batch' ? number :
R extends 'general:homepage_arbitrum_latest_batch' ? number :
R extends 'general:quick_search' ? Array<SearchResultItem> :
R extends 'general:search' ? SearchResult :
R extends 'general:search_check_redirect' ? SearchRedirectResult :
R extends 'general:config_backend' ? BackendConfig :
R extends 'general:config_backend_version' ? BackendVersionConfig :
R extends 'general:config_csv_export' ? CsvExportConfig :
R extends 'general:config_celo' ? CeloConfig :
R extends 'general:config_contract_languages' ? ContractLanguagesConfig :
R extends 'general:blob' ? Blob :
R extends 'general:validators_stability' ? ValidatorsStabilityResponse :
R extends 'general:validators_stability_counters' ? ValidatorsStabilityCountersResponse :
R extends 'general:validators_blackfort' ? ValidatorsBlackfortResponse :
R extends 'general:validators_blackfort_counters' ? ValidatorsBlackfortCountersResponse :
R extends 'general:validators_zilliqa' ? ValidatorsZilliqaResponse :
R extends 'general:validator_zilliqa' ? ValidatorZilliqa :
R extends 'general:epochs_celo' ? CeloEpochListResponse :
R extends 'general:epoch_celo' ? CeloEpochDetails :
R extends 'general:epoch_celo_election_rewards' ? CeloEpochElectionRewardDetailsResponse :
R extends 'general:user_ops' ? UserOpsResponse :
R extends 'general:user_op' ? UserOp :
R extends 'general:user_ops_account' ? UserOpsAccount :
R extends 'general:user_op_interpretation' ? TxInterpretationResponse :
R extends 'general:noves_transaction' ? NovesResponseData :
R extends 'general:noves_address_history' ? NovesAccountHistoryResponse :
R extends 'general:noves_describe_txs' ? NovesDescribeTxsResponse :
R extends 'general:withdrawals' ? WithdrawalsResponse :
R extends 'general:withdrawals_counters' ? WithdrawalsCounters :
R extends 'general:deposits' ? DepositsResponse :
R extends 'general:deposits_counters' ? DepositsCounters :
R extends 'general:advanced_filter' ? AdvancedFilterResponse :
R extends 'general:advanced_filter_methods' ? AdvancedFilterMethodsResponse :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiMiscPaginationFilters<R extends GeneralApiMiscResourceName> =
R extends 'general:stats_hot_contracts' ? HotContractsFilters :
R extends 'general:search' ? SearchResultFilters :
R extends 'general:user_ops' ? UserOpsFilters :
R extends 'general:validators_stability' ? ValidatorsStabilityFilters :
R extends 'general:advanced_filter' ? AdvancedFilterParams :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiMiscPaginationSorting<R extends GeneralApiMiscResourceName> =
R extends 'general:stats_hot_contracts' ? HotContractsSorting :
R extends 'general:validators_stability' ? ValidatorsStabilitySorting :
R extends 'general:validators_blackfort' ? ValidatorsBlackfortSorting :
never;
/* eslint-enable @stylistic/indent */
