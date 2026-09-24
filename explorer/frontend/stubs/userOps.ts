import type { UserOpsItem, UserOp, UserOpsAccount } from 'types/api/userOps';

import { ADDRESS_HASH } from './addressParams';
import { BLOCK_HASH } from './block';
import { TX_HASH } from './tx';

const USER_OP_HASH = '0xb94fab8f31f83001a23e20b2ce3cdcfb284c57a64b9a073e0e09c018bc701978';

export const USER_OPS_ITEM: UserOpsItem = {
  hash: USER_OP_HASH,
  block_number: '10356381',
  transaction_hash: TX_HASH,
  address: ADDRESS_HASH,
  timestamp: '2023-12-18T10:48:49.000000Z',
  status: true,
  fee: '48285720012071430',
};

export const USER_OP: UserOp = {
  hash: USER_OP_HASH,
  sender: ADDRESS_HASH,
  nonce: '0x00b',
  call_data: '0x123',
  execute_call_data: null,
  decoded_call_data: null,
  decoded_execute_call_data: null,
  call_gas_limit: '71316',
  verification_gas_limit: '91551',
  pre_verification_gas: '53627',
  max_fee_per_gas: '100000020',
  max_priority_fee_per_gas: '100000000',
  signature: '0x000',
  aggregator: null,
  aggregator_signature: null,
  entry_point: ADDRESS_HASH,
  transaction_hash: TX_HASH,
  block_number: '10358181',
  block_hash: BLOCK_HASH,
  bundler: ADDRESS_HASH,
  factory: null,
  paymaster: ADDRESS_HASH,
  status: true,
  revert_reason: null,
  gas: '399596',
  gas_price: '1575000898',
  gas_used: '118810',
  sponsor_type: 'paymaster_sponsor',
  fee: '17927001792700',
  timestamp: '2023-12-18T10:48:49.000000Z',
  user_logs_count: 1,
  user_logs_start_index: 2,
  raw: {
    sender: ADDRESS_HASH,
    nonce: '1',
    init_code: '0x',
    call_data: '0x345',
    call_gas_limit: '29491',
    verification_gas_limit: '80734',
    pre_verification_gas: '3276112',
    max_fee_per_gas: '309847206',
    max_priority_fee_per_gas: '100000000',
    paymaster_and_data: '0x',
    signature: '0x000',
  },
};

export const USER_OPS_ACCOUNT: UserOpsAccount = {
  total_ops: 1,
};
