import type { ZkSyncBatch, ZkSyncBatchesItem } from 'types/api/zkSyncL2';

import { TX_HASH } from './tx';

export const ZKSYNC_L2_TXN_BATCHES_ITEM: ZkSyncBatchesItem = {
  commit_transaction_hash: TX_HASH,
  commit_transaction_timestamp: '2022-03-17T19:33:04.519145Z',
  execute_transaction_hash: TX_HASH,
  execute_transaction_timestamp: '2022-03-17T20:49:48.856345Z',
  number: 8002,
  prove_transaction_hash: TX_HASH,
  prove_transaction_timestamp: '2022-03-17T20:49:48.772442Z',
  status: 'Executed on L1',
  timestamp: '2022-03-17T17:00:11.000000Z',
  transactions_count: 1215,
};

export const ZKSYNC_L2_TXN_BATCH: ZkSyncBatch = {
  ...ZKSYNC_L2_TXN_BATCHES_ITEM,
  start_block_number: 1245209,
  end_block_number: 1245490,
  l1_gas_price: '4173068062',
  l1_transactions_count: 0,
  l2_fair_gas_price: '100000000',
  l2_transactions_count: 287,
  root_hash: '0x108c635b94f941fcabcb85500daec2f6be4f0747dff649b1cdd9dd7a7a264792',
};
