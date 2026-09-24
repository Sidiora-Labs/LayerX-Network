import type { ApiResource } from '../../types';
import type {
  AddressMudTables,
  AddressMudTablesFilter,
  AddressMudRecords,
  AddressMudRecordsFilter,
  AddressMudRecordsSorting,
  AddressMudRecord,
} from 'types/api/address';
import type {
  ArbitrumL2MessagesResponse,
  ArbitrumL2TxnBatch,
  ArbitrumL2TxnBatchesResponse,
  ArbitrumL2BatchTxs,
  ArbitrumL2BatchBlocks,
  ArbitrumL2TxnWithdrawalsResponse,
  ArbitrumL2MessageClaimResponse,
} from 'types/api/arbitrumL2';
import type {
  SmartContractMudSystemsResponse,
  SmartContractMudSystemInfo,
} from 'types/api/contract';
import type { InteropMessageListResponse } from 'types/api/interop';
import type { MudWorldsResponse } from 'types/api/mudWorlds';
import type {
  OptimisticL2DepositsResponse,
  OptimisticL2OutputRootsResponse,
  OptimisticL2TxnBatchesResponse,
  OptimisticL2WithdrawalsResponse,
  OptimisticL2DisputeGamesResponse,
  OptimismL2TxnBatch,
  OptimismL2BatchTxs,
  OptimismL2BatchBlocks,
} from 'types/api/optimisticL2';
import type {
  ScrollL2BatchesResponse,
  ScrollL2TxnBatch,
  ScrollL2TxnBatchTxs,
  ScrollL2TxnBatchBlocks,
  ScrollL2MessagesResponse,
} from 'types/api/scrollL2';
import type { ShibariumWithdrawalsResponse, ShibariumDepositsResponse } from 'types/api/shibarium';
import type {
  ZkEvmL2DepositsResponse,
  ZkEvmL2TxnBatch,
  ZkEvmL2TxnBatchesResponse,
  ZkEvmL2TxnBatchTxs,
  ZkEvmL2WithdrawalsResponse,
} from 'types/api/zkEvmL2';
import type { ZkSyncBatch, ZkSyncBatchesResponse, ZkSyncBatchTxs } from 'types/api/zkSyncL2';

export const GENERAL_API_ROLLUP_RESOURCES = {
  // OPTIMISTIC
  optimistic_l2_deposits: {
    path: '/api/v2/optimism/deposits',
    filterFields: [],
    paginated: true,
  },
  optimistic_l2_deposits_count: {
    path: '/api/v2/optimism/deposits/count',
  },
  optimistic_l2_withdrawals: {
    path: '/api/v2/optimism/withdrawals',
    filterFields: [],
    paginated: true,
  },
  optimistic_l2_withdrawals_count: {
    path: '/api/v2/optimism/withdrawals/count',
  },
  optimistic_l2_output_roots: {
    path: '/api/v2/optimism/output-roots',
    filterFields: [],
    paginated: true,
  },
  optimistic_l2_output_roots_count: {
    path: '/api/v2/optimism/output-roots/count',
  },
  optimistic_l2_txn_batches: {
    path: '/api/v2/optimism/batches',
    filterFields: [],
    paginated: true,
  },
  optimistic_l2_txn_batches_count: {
    path: '/api/v2/optimism/batches/count',
  },
  optimistic_l2_txn_batch: {
    path: '/api/v2/optimism/batches/:number',
    pathParams: [ 'number' as const ],
  },
  optimistic_l2_txn_batch_celestia: {
    path: '/api/v2/optimism/batches/da/celestia/:height/:commitment',
    pathParams: [ 'height' as const, 'commitment' as const ],
  },
  optimistic_l2_txn_batch_txs: {
    path: '/api/v2/transactions/optimism-batch/:number',
    pathParams: [ 'number' as const ],
    filterFields: [],
    paginated: true,
  },
  optimistic_l2_txn_batch_blocks: {
    path: '/api/v2/blocks/optimism-batch/:number',
    pathParams: [ 'number' as const ],
    filterFields: [],
    paginated: true,
  },
  optimistic_l2_dispute_games: {
    path: '/api/v2/optimism/games',
    filterFields: [],
    paginated: true,
  },
  optimistic_l2_dispute_games_count: {
    path: '/api/v2/optimism/games/count',
  },

  // OPTIMISTIC INTEROP
  optimistic_l2_interop_messages: {
    path: '/api/v2/optimism/interop/messages',
    filterFields: [],
    paginated: true,
  },
  optimistic_l2_interop_messages_count: {
    path: '/api/v2/optimism/interop/messages/count',
  },

  // MUD
  mud_worlds: {
    path: '/api/v2/mud/worlds',
    filterFields: [],
    paginated: true,
  },
  mud_tables: {
    path: '/api/v2/mud/worlds/:hash/tables',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'q' as const ],
    paginated: true,
  },
  mud_tables_count: {
    path: '/api/v2/mud/worlds/:hash/tables/count',
    pathParams: [ 'hash' as const ],
  },
  mud_records: {
    path: '/api/v2/mud/worlds/:hash/tables/:table_id/records',
    pathParams: [ 'hash' as const, 'table_id' as const ],
    filterFields: [ 'filter_key0' as const, 'filter_key1' as const ],
    paginated: true,
  },
  mud_record: {
    path: '/api/v2/mud/worlds/:hash/tables/:table_id/records/:record_id',
    pathParams: [ 'hash' as const, 'table_id' as const, 'record_id' as const ],
  },
  mud_systems: {
    path: '/api/v2/mud/worlds/:hash/systems',
    pathParams: [ 'hash' as const ],
  },
  mud_system_info: {
    path: '/api/v2/mud/worlds/:hash/systems/:system_address',
    pathParams: [ 'hash' as const, 'system_address' as const ],
  },

  // ARBITRUM
  arbitrum_l2_messages: {
    path: '/api/v2/arbitrum/messages/:direction',
    pathParams: [ 'direction' as const ],
    filterFields: [],
    paginated: true,
  },
  arbitrum_l2_messages_count: {
    path: '/api/v2/arbitrum/messages/:direction/count',
    pathParams: [ 'direction' as const ],
  },
  arbitrum_l2_txn_batches: {
    path: '/api/v2/arbitrum/batches',
    filterFields: [],
    paginated: true,
  },
  arbitrum_l2_txn_batches_count: {
    path: '/api/v2/arbitrum/batches/count',
  },
  arbitrum_l2_txn_batch: {
    path: '/api/v2/arbitrum/batches/:number',
    pathParams: [ 'number' as const ],
  },
  arbitrum_l2_txn_batch_celestia: {
    path: '/api/v2/arbitrum/batches/da/celestia/:height/:commitment',
    pathParams: [ 'height' as const, 'commitment' as const ],
  },
  arbitrum_l2_txn_batch_txs: {
    path: '/api/v2/transactions/arbitrum-batch/:number',
    pathParams: [ 'number' as const ],
    filterFields: [],
    paginated: true,
  },
  arbitrum_l2_txn_batch_blocks: {
    path: '/api/v2/blocks/arbitrum-batch/:number',
    pathParams: [ 'number' as const ],
    filterFields: [],
    paginated: true,
  },
  arbitrum_l2_txn_withdrawals: {
    path: '/api/v2/arbitrum/messages/withdrawals/:hash',
    pathParams: [ 'hash' as const ],
    filterFields: [],
  },
  arbitrum_l2_message_claim: {
    path: '/api/v2/arbitrum/messages/claim/:id',
    pathParams: [ 'id' as const ],
    filterFields: [],
  },

  // zkSync
  zksync_l2_txn_batches: {
    path: '/api/v2/zksync/batches',
    filterFields: [],
    paginated: true,
  },
  zksync_l2_txn_batches_count: {
    path: '/api/v2/zksync/batches/count',
  },
  zksync_l2_txn_batch: {
    path: '/api/v2/zksync/batches/:number',
    pathParams: [ 'number' as const ],
  },
  zksync_l2_txn_batch_txs: {
    path: '/api/v2/transactions/zksync-batch/:number',
    pathParams: [ 'number' as const ],
    filterFields: [],
    paginated: true,
  },

  // zkEvm
  zkevm_l2_deposits: {
    path: '/api/v2/zkevm/deposits',
    filterFields: [],
    paginated: true,
  },
  zkevm_l2_deposits_count: {
    path: '/api/v2/zkevm/deposits/count',
  },
  zkevm_l2_withdrawals: {
    path: '/api/v2/zkevm/withdrawals',
    filterFields: [],
    paginated: true,
  },
  zkevm_l2_withdrawals_count: {
    path: '/api/v2/zkevm/withdrawals/count',
  },
  zkevm_l2_txn_batches: {
    path: '/api/v2/zkevm/batches',
    filterFields: [],
    paginated: true,
  },
  zkevm_l2_txn_batches_count: {
    path: '/api/v2/zkevm/batches/count',
  },
  zkevm_l2_txn_batch: {
    path: '/api/v2/zkevm/batches/:number',
    pathParams: [ 'number' as const ],
  },
  zkevm_l2_txn_batch_txs: {
    path: '/api/v2/transactions/zkevm-batch/:number',
    pathParams: [ 'number' as const ],
    filterFields: [],
    paginated: true,
  },

  // SHIBARIUM
  shibarium_deposits: {
    path: '/api/v2/shibarium/deposits',
    filterFields: [],
    paginated: true,
  },
  shibarium_deposits_count: {
    path: '/api/v2/shibarium/deposits/count',
  },
  shibarium_withdrawals: {
    path: '/api/v2/shibarium/withdrawals',
    filterFields: [],
    paginated: true,
  },
  shibarium_withdrawals_count: {
    path: '/api/v2/shibarium/withdrawals/count',
  },

  // SCROLL
  scroll_l2_deposits: {
    path: '/api/v2/scroll/deposits',
    filterFields: [],
    paginated: true,
  },
  scroll_l2_deposits_count: {
    path: '/api/v2/scroll/deposits/count',
  },
  scroll_l2_withdrawals: {
    path: '/api/v2/scroll/withdrawals',
    filterFields: [],
    paginated: true,
  },
  scroll_l2_withdrawals_count: {
    path: '/api/v2/scroll/withdrawals/count',
  },
  scroll_l2_txn_batches: {
    path: '/api/v2/scroll/batches',
    filterFields: [],
    paginated: true,
  },
  scroll_l2_txn_batches_count: {
    path: '/api/v2/scroll/batches/count',
  },
  scroll_l2_txn_batch: {
    path: '/api/v2/scroll/batches/:number',
    pathParams: [ 'number' as const ],
  },
  scroll_l2_txn_batch_txs: {
    path: '/api/v2/transactions/scroll-batch/:number',
    pathParams: [ 'number' as const ],
    filterFields: [],
    paginated: true,
  },
  scroll_l2_txn_batch_blocks: {
    path: '/api/v2/blocks/scroll-batch/:number',
    pathParams: [ 'number' as const ],
    filterFields: [],
    paginated: true,
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiRollupResourceName = `general:${ keyof typeof GENERAL_API_ROLLUP_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiRollupResourcePayload<R extends GeneralApiRollupResourceName> =
R extends 'general:optimistic_l2_output_roots' ? OptimisticL2OutputRootsResponse :
R extends 'general:optimistic_l2_withdrawals' ? OptimisticL2WithdrawalsResponse :
R extends 'general:optimistic_l2_deposits' ? OptimisticL2DepositsResponse :
R extends 'general:optimistic_l2_txn_batches' ? OptimisticL2TxnBatchesResponse :
R extends 'general:optimistic_l2_txn_batches_count' ? number :
R extends 'general:optimistic_l2_txn_batch' ? OptimismL2TxnBatch :
R extends 'general:optimistic_l2_txn_batch_celestia' ? OptimismL2TxnBatch :
R extends 'general:optimistic_l2_txn_batch_txs' ? OptimismL2BatchTxs :
R extends 'general:optimistic_l2_txn_batch_blocks' ? OptimismL2BatchBlocks :
R extends 'general:optimistic_l2_dispute_games' ? OptimisticL2DisputeGamesResponse :
R extends 'general:optimistic_l2_output_roots_count' ? number :
R extends 'general:optimistic_l2_withdrawals_count' ? number :
R extends 'general:optimistic_l2_deposits_count' ? number :
R extends 'general:optimistic_l2_dispute_games_count' ? number :
R extends 'general:optimistic_l2_interop_messages' ? InteropMessageListResponse :
R extends 'general:optimistic_l2_interop_messages_count' ? number :
R extends 'general:shibarium_withdrawals' ? ShibariumWithdrawalsResponse :
R extends 'general:shibarium_deposits' ? ShibariumDepositsResponse :
R extends 'general:shibarium_withdrawals_count' ? number :
R extends 'general:shibarium_deposits_count' ? number :
R extends 'general:arbitrum_l2_messages' ? ArbitrumL2MessagesResponse :
R extends 'general:arbitrum_l2_messages_count' ? number :
R extends 'general:arbitrum_l2_txn_batches' ? ArbitrumL2TxnBatchesResponse :
R extends 'general:arbitrum_l2_txn_batches_count' ? number :
R extends 'general:arbitrum_l2_txn_batch' ? ArbitrumL2TxnBatch :
R extends 'general:arbitrum_l2_txn_batch_celestia' ? ArbitrumL2TxnBatch :
R extends 'general:arbitrum_l2_txn_batch_txs' ? ArbitrumL2BatchTxs :
R extends 'general:arbitrum_l2_txn_batch_blocks' ? ArbitrumL2BatchBlocks :
R extends 'general:arbitrum_l2_txn_withdrawals' ? ArbitrumL2TxnWithdrawalsResponse :
R extends 'general:arbitrum_l2_message_claim' ? ArbitrumL2MessageClaimResponse :
R extends 'general:zkevm_l2_deposits' ? ZkEvmL2DepositsResponse :
R extends 'general:zkevm_l2_deposits_count' ? number :
R extends 'general:zkevm_l2_withdrawals' ? ZkEvmL2WithdrawalsResponse :
R extends 'general:zkevm_l2_withdrawals_count' ? number :
R extends 'general:zkevm_l2_txn_batches' ? ZkEvmL2TxnBatchesResponse :
R extends 'general:zkevm_l2_txn_batches_count' ? number :
R extends 'general:zkevm_l2_txn_batch' ? ZkEvmL2TxnBatch :
R extends 'general:zkevm_l2_txn_batch_txs' ? ZkEvmL2TxnBatchTxs :
R extends 'general:zksync_l2_txn_batches' ? ZkSyncBatchesResponse :
R extends 'general:zksync_l2_txn_batches_count' ? number :
R extends 'general:zksync_l2_txn_batch' ? ZkSyncBatch :
R extends 'general:zksync_l2_txn_batch_txs' ? ZkSyncBatchTxs :
R extends 'general:scroll_l2_txn_batch_txs' ? ScrollL2TxnBatchTxs :
R extends 'general:scroll_l2_txn_batch_blocks' ? ScrollL2TxnBatchBlocks :
R extends 'general:scroll_l2_txn_batches' ? ScrollL2BatchesResponse :
R extends 'general:scroll_l2_txn_batches_count' ? number :
R extends 'general:scroll_l2_txn_batch' ? ScrollL2TxnBatch :
R extends 'general:scroll_l2_deposits' ? ScrollL2MessagesResponse :
R extends 'general:scroll_l2_deposits_count' ? number :
R extends 'general:scroll_l2_withdrawals' ? ScrollL2MessagesResponse :
R extends 'general:scroll_l2_withdrawals_count' ? number :
R extends 'general:mud_worlds' ? MudWorldsResponse :
R extends 'general:mud_tables' ? AddressMudTables :
R extends 'general:mud_tables_count' ? number :
R extends 'general:mud_records' ? AddressMudRecords :
R extends 'general:mud_record' ? AddressMudRecord :
R extends 'general:mud_systems' ? SmartContractMudSystemsResponse :
R extends 'general:mud_system_info' ? SmartContractMudSystemInfo :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiRollupPaginationFilters<R extends GeneralApiRollupResourceName> =
R extends 'general:mud_tables' ? AddressMudTablesFilter :
R extends 'general:mud_records' ? AddressMudRecordsFilter :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiRollupPaginationSorting<R extends GeneralApiRollupResourceName> =
R extends 'general:mud_records' ? AddressMudRecordsSorting :
never;
/* eslint-enable @stylistic/indent */
