import type * as stats from '@blockscout/stats-types';
import type { RawTracesResponse } from 'types/api/rawTrace';
import type { Transaction, TransactionsStats } from 'types/api/transaction';

import { ADDRESS_PARAMS } from './addressParams';
import { STATS_COUNTER } from './stats';

export const TX_HASH = '0x3ed9d81e7c1001bdda1caa1dc62c0acbbe3d2c671cdc20dc1e65efdaa4186967';

export const TX: Transaction = {
  timestamp: '2022-11-11T11:11:11.000000Z',
  fee: {
    type: 'actual',
    value: '2100000000000000',
  },
  gas_limit: '21000',
  block_number: 9004925,
  status: 'ok',
  method: 'placeholder',
  confirmations: 71,
  type: 0,
  exchange_rate: '1828.71',
  historic_exchange_rate: null,
  to: ADDRESS_PARAMS,
  transaction_burnt_fee: null,
  max_fee_per_gas: null,
  result: 'success',
  hash: '0x2b824349b320cfa72f292ab26bf525adb00083ba9fa097141896c3c8c74567cc',
  gas_price: '100000000000',
  priority_fee: null,
  base_fee_per_gas: '24',
  from: ADDRESS_PARAMS,
  token_transfers: null,
  transaction_types: [
    'coin_transfer',
  ],
  gas_used: '21000',
  created_contract: null,
  position: 0,
  nonce: 295929,
  has_error_in_internal_transactions: false,
  actions: [],
  decoded_input: null,
  token_transfers_overflow: false,
  raw_input: '0x',
  value: '42000420000000000000',
  max_priority_fee_per_gas: null,
  revert_reason: null,
  confirmation_duration: [
    0,
    14545,
  ],
  transaction_tag: null,
};

export const TX_ZKEVM_L2: Transaction = {
  ...TX,
  zkevm_batch_number: 12345,
  zkevm_sequence_hash: '0x2b824349b320cfa72f292ab26bf525adb00083ba9fa097141896c3c8c74567cc',
  zkevm_status: 'Confirmed by Sequencer',
  zkevm_verify_hash: '0x2b824349b320cfa72f292ab26bf525adb00083ba9fa097141896c3c8c74567cc',
};

export const TX_RAW_TRACE: RawTracesResponse = [];

export const TXS_STATS: TransactionsStats = {
  pending_transactions_count: '4200',
  transaction_fees_avg_24h: '22342870314428',
  transaction_fees_sum_24h: '22184012506492688277',
  transactions_count_24h: '992890',
};

export const TXS_STATS_MICROSERVICE: stats.TransactionsPageStats = {
  pending_transactions_30m: STATS_COUNTER,
  transactions_24h: STATS_COUNTER,
  operational_transactions_24h: STATS_COUNTER,
  transactions_fee_24h: STATS_COUNTER,
  average_transactions_fee_24h: STATS_COUNTER,
};
