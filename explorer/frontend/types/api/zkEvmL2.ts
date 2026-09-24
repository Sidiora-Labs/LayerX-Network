import type { Transaction } from './transaction';

export type ZkEvmL2DepositsItem = {
  block_number: number;
  index: number;
  l1_transaction_hash: string;
  l2_transaction_hash: string | null;
  timestamp: string;
  value: string;
  symbol: string;
};

export type ZkEvmL2DepositsResponse = {
  items: Array<ZkEvmL2DepositsItem>;
  next_page_params: {
    items_count: number;
    index: number;
  };
};

export type ZkEvmL2WithdrawalsItem = {
  block_number: number;
  index: number;
  l1_transaction_hash: string | null;
  l2_transaction_hash: string;
  timestamp: string;
  value: string;
  symbol: string;
};

export type ZkEvmL2WithdrawalsResponse = {
  items: Array<ZkEvmL2WithdrawalsItem>;
  next_page_params: {
    items_count: number;
    index: number;
  };
};

export type ZkEvmL2TxnBatchesItem = {
  number: number;
  verify_transaction_hash: string | null;
  sequence_transaction_hash: string | null;
  status: string;
  timestamp: string | null;
  transactions_count: number;
};

export type ZkEvmL2TxnBatchesResponse = {
  items: Array<ZkEvmL2TxnBatchesItem>;
  next_page_params: {
    number: number;
    items_count: number;
  } | null;
};

export const ZKEVM_L2_TX_BATCH_STATUSES = [ 'Unfinalized', 'L1 Sequence Confirmed', 'Finalized' ];

export type ZkEvmL2TxnBatch = {
  acc_input_hash: string;
  global_exit_root: string;
  number: number;
  sequence_transaction_hash: string;
  state_root: string;
  status: typeof ZKEVM_L2_TX_BATCH_STATUSES[number];
  timestamp: string | null;
  transactions: Array<string>;
  verify_transaction_hash: string;
};

export type ZkEvmL2TxnBatchTxs = {
  items: Array<Transaction>;
  // API response doesn't have next_page_params option, but we need to add it to the type for consistency
  next_page_params: null;
};

export type NewZkEvmBatchSocketResponse = { batch: ZkEvmL2TxnBatchesItem };
