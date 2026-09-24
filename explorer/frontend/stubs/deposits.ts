import type { DepositsItem } from 'types/api/deposits';

import { ADDRESS_PARAMS } from './addressParams';
import { TX_HASH } from './tx';

export const DEPOSIT: DepositsItem = {
  amount: '12565723',
  index: 1,
  block_number: 1231111111,
  block_hash: '0x1234567890',
  block_timestamp: '2023-05-12T19:29:12.000000Z',
  pubkey: '0x1234567890123456789012345678901234567890',
  status: 'pending',
  from_address: ADDRESS_PARAMS,
  transaction_hash: TX_HASH,
  withdrawal_address: ADDRESS_PARAMS,
  signature: '0x1234567890123456789012345678901234567890',
};
