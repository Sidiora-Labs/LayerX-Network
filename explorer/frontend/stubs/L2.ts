import type {
  OptimismL2TxnBatch,
  OptimisticL2DepositsItem,
  OptimisticL2DisputeGamesItem,
  OptimisticL2OutputRootsItem,
  OptimisticL2TxnBatchesItem,
  OptimisticL2WithdrawalsItem,
} from 'types/api/optimisticL2';

import { ADDRESS_HASH, ADDRESS_PARAMS } from './addressParams';
import { TX_HASH } from './tx';

export const L2_DEPOSIT_ITEM: OptimisticL2DepositsItem = {
  l1_block_number: 9045233,
  l1_block_timestamp: '2023-05-22T18:00:36.000000Z',
  l1_transaction_hash: TX_HASH,
  l1_transaction_origin: ADDRESS_HASH,
  l2_transaction_gas_limit: '100000',
  l2_transaction_hash: TX_HASH,
};

export const L2_WITHDRAWAL_ITEM: OptimisticL2WithdrawalsItem = {
  challenge_period_end: null,
  from: ADDRESS_PARAMS,
  l1_transaction_hash: TX_HASH,
  l2_timestamp: '2023-06-01T13:44:56.000000Z',
  l2_transaction_hash: TX_HASH,
  msg_nonce: 2393,
  msg_nonce_version: 1,
  status: 'Ready to prove',
  portal_contract_address_hash: null,
  msg_sender_address_hash: null,
  msg_target_address_hash: null,
  msg_data: null,
  msg_gas_limit: null,
  msg_nonce_raw: null,
  msg_value: null,
};

export const L2_TXN_BATCHES_ITEM: OptimisticL2TxnBatchesItem = {
  number: 260991,
  batch_data_container: 'in_blob4844',
  l1_timestamp: '2023-06-01T14:46:48.000000Z',
  l1_transaction_hashes: [
    TX_HASH,
  ],
  l2_start_block_number: 5218590,
  l2_end_block_number: 5218777,
  transactions_count: 9,
};

export const L2_TXN_BATCH: OptimismL2TxnBatch = {
  ...L2_TXN_BATCHES_ITEM,
  batch_data_container: 'in_blob4844',
  blobs: [
    {
      hash: '0x01fb41e1ae9f827e13abb0ee94be2ee574a23ac31426cea630ddd18af854bc85',
      l1_timestamp: '2024-09-03T13:26:23.000000Z',
      l1_transaction_hash: '0xd25ee571f1701690615099b208a9431d8611d0130dc342bead6d9edc291f04b9',
    },
  ],
};

export const L2_OUTPUT_ROOTS_ITEM: OptimisticL2OutputRootsItem = {
  l1_block_number: 9103684,
  l1_timestamp: '2023-06-01T15:26:12.000000Z',
  l1_transaction_hash: TX_HASH,
  l2_block_number: 10102468,
  l2_output_index: 50655,
  output_root: TX_HASH,
};

export const L2_DISPUTE_GAMES_ITEM: OptimisticL2DisputeGamesItem = {
  contract_address_hash: ADDRESS_HASH,
  created_at: '2023-06-01T15:26:12.000000Z',
  game_type: 0,
  index: 6594,
  l2_block_number: 50655,
  resolved_at: null,
  status: 'In progress',
};
