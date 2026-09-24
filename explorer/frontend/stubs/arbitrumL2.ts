import type { ArbitrumL2TxnBatchesItem, ArbitrumL2TxnBatch, ArbitrumL2MessagesItem, ArbitrumL2TxnWithdrawalsItem } from 'types/api/arbitrumL2';

import { ADDRESS_HASH } from './addressParams';
import { TX_HASH } from './tx';

export const ARBITRUM_MESSAGES_ITEM: ArbitrumL2MessagesItem = {
  completion_transaction_hash: TX_HASH,
  id: 181920,
  origination_address_hash: ADDRESS_HASH,
  origination_transaction_block_number: 123456,
  origination_transaction_hash: TX_HASH,
  origination_timestamp: '2023-06-01T14:46:48.000000Z',
  status: 'relayed',
};

export const ARBITRUM_L2_TXN_BATCHES_ITEM: ArbitrumL2TxnBatchesItem = {
  number: 12345,
  blocks_count: 12345,
  transactions_count: 10000,
  commitment_transaction: {
    block_number: 12345,
    timestamp: '2024-04-17T08:51:58.000000Z',
    hash: TX_HASH,
    status: 'finalized',
  },
  batch_data_container: 'in_blob4844',
};

export const ARBITRUM_L2_TXN_BATCH: ArbitrumL2TxnBatch = {
  ...ARBITRUM_L2_TXN_BATCHES_ITEM,
  after_acc_hash: '0xcd064f3409015e8e6407e492e5275a185e492c6b43ccf127f22092d8057a9ffb',
  before_acc_hash: '0x2ed7c4985eb778d76ec400a43805e7feecc8c2afcdb492dbe5caf227de6d37bc',
  start_block_number: 1245209,
  end_block_number: 1245490,
  data_availability: {
    batch_data_container: 'in_blob4844',
  },
};

export const ARBITRUM_L2_TXN_WITHDRAWALS_ITEM: ArbitrumL2TxnWithdrawalsItem = {
  arb_block_number: 70889261,
  caller_address_hash: '0x507f55d716340fc836ba52c1a8daebcfeedeef1a',
  completion_transaction_hash: null,
  callvalue: '100000000000000',
  data: '0x',
  destination_address_hash: '0x507f55d716340fc836ba52c1a8daebcfeedeef1a',
  eth_block_number: 6494128,
  id: 43685,
  l2_timestamp: 1723578569,
  status: 'relayed',
  token: null,
};
