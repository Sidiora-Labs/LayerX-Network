import type { ApiResource } from '../../types';
import type { TxBlobs } from 'types/api/blobs';
import type { FheOperationsResponse } from 'types/api/fheOperations';
import type { InternalTransactionFilters, InternalTransactionsResponse } from 'types/api/internalTransaction';
import type { LogsResponseTx } from 'types/api/log';
import type { RawTracesResponse } from 'types/api/rawTrace';
import type { TokenTransferResponse, TokenTransferFilters } from 'types/api/tokenTransfer';
import type {
  TransactionsResponseValidated,
  TransactionsResponsePending,
  Transaction,
  TransactionsResponseWatchlist,
  TransactionsResponseWithBlobs,
  TransactionsStats,
} from 'types/api/transaction';
import type { TxInterpretationResponse } from 'types/api/txInterpretation';
import type { TTxsFilters, TTxsWithBlobsFilters } from 'types/api/txsFilters';
import type { TxStateChanges } from 'types/api/txStateChanges';

export const GENERAL_API_TX_RESOURCES = {
  txs_stats: {
    path: '/api/v2/transactions/stats',
  },
  txs_validated: {
    path: '/api/v2/transactions',
    filterFields: [ 'filter' as const, 'type' as const, 'method' as const ],
    paginated: true,
  },
  txs_pending: {
    path: '/api/v2/transactions',
    filterFields: [ 'filter' as const, 'type' as const, 'method' as const ],
    paginated: true,
  },
  txs_with_blobs: {
    path: '/api/v2/transactions',
    filterFields: [ 'type' as const ],
    paginated: true,
  },
  txs_watchlist: {
    path: '/api/v2/transactions/watchlist',
    filterFields: [ ],
    paginated: true,
  },
  txs_execution_node: {
    path: '/api/v2/transactions/execution-node/:hash',
    pathParams: [ 'hash' as const ],
    filterFields: [ ],
    paginated: true,
  },
  tx: {
    path: '/api/v2/transactions/:hash',
    pathParams: [ 'hash' as const ],
  },
  tx_internal_txs: {
    path: '/api/v2/transactions/:hash/internal-transactions',
    pathParams: [ 'hash' as const ],
    filterFields: [ ],
    paginated: true,
  },
  tx_logs: {
    path: '/api/v2/transactions/:hash/logs',
    pathParams: [ 'hash' as const ],
    filterFields: [ ],
    paginated: true,
  },
  tx_fhe_operations: {
    path: '/api/v2/transactions/:hash/fhe-operations',
    pathParams: [ 'hash' as const ],
  },

  tx_token_transfers: {
    path: '/api/v2/transactions/:hash/token-transfers',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'type' as const ],
    paginated: true,
  },
  tx_raw_trace: {
    path: '/api/v2/transactions/:hash/raw-trace',
    pathParams: [ 'hash' as const ],
  },
  tx_state_changes: {
    path: '/api/v2/transactions/:hash/state-changes',
    pathParams: [ 'hash' as const ],
    filterFields: [],
    paginated: true,
  },
  tx_blobs: {
    path: '/api/v2/transactions/:hash/blobs',
    pathParams: [ 'hash' as const ],
    paginated: true,
  },
  tx_interpretation: {
    path: '/api/v2/transactions/:hash/summary',
    pathParams: [ 'hash' as const ],
  },
  tx_external_transactions: {
    path: '/api/v2/transactions/:hash/external-transactions',
    pathParams: [ 'hash' as const ],
  },
  internal_txs: {
    path: '/api/v2/internal-transactions',
    paginated: true,
    filterFields: [ 'transaction_hash' as const ],
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiTxResourceName = `general:${ keyof typeof GENERAL_API_TX_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiTxResourcePayload<R extends GeneralApiTxResourceName> =
R extends 'general:txs_stats' ? TransactionsStats :
R extends 'general:txs_validated' ? TransactionsResponseValidated :
R extends 'general:txs_pending' ? TransactionsResponsePending :
R extends 'general:txs_with_blobs' ? TransactionsResponseWithBlobs :
R extends 'general:txs_watchlist' ? TransactionsResponseWatchlist :
R extends 'general:txs_execution_node' ? TransactionsResponseValidated :
R extends 'general:tx_internal_txs' ? InternalTransactionsResponse :
R extends 'general:tx' ? Transaction :
R extends 'general:tx_logs' ? LogsResponseTx :
R extends 'general:tx_token_transfers' ? TokenTransferResponse :
R extends 'general:tx_fhe_operations' ? FheOperationsResponse :
R extends 'general:tx_raw_trace' ? RawTracesResponse :
R extends 'general:tx_state_changes' ? TxStateChanges :
R extends 'general:tx_blobs' ? TxBlobs :
R extends 'general:tx_interpretation' ? TxInterpretationResponse :
R extends 'general:tx_external_transactions' ? Array<string> :
R extends 'general:internal_txs' ? InternalTransactionsResponse :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type GeneralApiTxPaginationFilters<R extends GeneralApiTxResourceName> =
R extends 'general:txs_validated' | 'general:txs_pending' ? TTxsFilters :
R extends 'general:txs_with_blobs' ? TTxsWithBlobsFilters :
R extends 'general:tx_token_transfers' ? TokenTransferFilters :
R extends 'general:internal_txs' ? InternalTransactionFilters :
never;
/* eslint-enable @stylistic/indent */
