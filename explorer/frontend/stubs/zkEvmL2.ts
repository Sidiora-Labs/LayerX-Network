import type { ZkEvmL2DepositsItem, ZkEvmL2TxnBatch, ZkEvmL2TxnBatchesItem, ZkEvmL2WithdrawalsItem } from 'types/api/zkEvmL2';

import { TX_HASH } from './tx';

export const ZKEVM_DEPOSITS_ITEM: ZkEvmL2DepositsItem = {
  block_number: 19674901,
  index: 181920,
  l1_transaction_hash: '0xa74edfa5824a07a5f95ca1145140ed589df7f05bb17796bf18090b14c4566b5d',
  l2_transaction_hash: '0x436d1c7ada270466ca0facdb96ecc22934d68d13b8a08f541b8df11b222967b5',
  symbol: 'ETH',
  timestamp: '2023-06-01T14:46:48.000000Z',
  value: '0.13040262',
};

export const ZKEVM_WITHDRAWALS_ITEM: ZkEvmL2WithdrawalsItem = {
  block_number: 11692968,
  index: 47003,
  l1_transaction_hash: '0x230cf46dabea287ac7d0ba83b8ea120bb83c1de58a81d34f44788f0459096c52',
  l2_transaction_hash: '0x519d9f025ec47f08a48d708964d177189d2246ddf988686c481f5debcf097e34',
  symbol: 'ETH',
  timestamp: '2024-04-17T08:51:58.000000Z',
  value: '110.35',
};

export const ZKEVM_L2_TXN_BATCHES_ITEM: ZkEvmL2TxnBatchesItem = {
  timestamp: '2023-06-01T14:46:48.000000Z',
  status: 'Finalized',
  verify_transaction_hash: TX_HASH,
  sequence_transaction_hash: TX_HASH,
  number: 5218590,
  transactions_count: 9,
};

export const ZKEVM_L2_TXN_BATCH: ZkEvmL2TxnBatch = {
  acc_input_hash: '0xb815fe2832977f1324ad0124a019b938f189f7b470292f40a21284f15774b3b3',
  global_exit_root: '0x0000000000000000000000000000000000000000000000000000000000000000',
  number: 1,
  sequence_transaction_hash: '0x57b9b95db5f94f125710bdc8fbb3fabaac10125b44b0cb61dbc69daddf06d0cd',
  state_root: '0xb9a589d6b3ae44d3b250a9993caa5e3721568197f56e4743989ecb2285d80ec4',
  status: 'Finalized',
  timestamp: '2023-09-15T06:22:48.000000Z',
  transactions: [ '0xff99dd67646b8f3d657cc6f19eb33abc346de2dbaccd03e45e7726cc28e3e186' ],
  verify_transaction_hash: '0x093276fa65c67d7b12dd96f4fefafba9d9ad2f1c23c6e53f96583971ce75352d',
};
