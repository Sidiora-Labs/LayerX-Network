import type { InternalTransaction } from 'types/api/internalTransaction';

import { ADDRESS_PARAMS } from './addressParams';
import { TX_HASH } from './tx';

export const INTERNAL_TX: InternalTransaction = {
  block_number: 9006105,
  created_contract: null,
  error: null,
  from: ADDRESS_PARAMS,
  gas_limit: '754278',
  index: 1,
  success: true,
  timestamp: '2023-05-15T20:14:00.000000Z',
  to: ADDRESS_PARAMS,
  transaction_hash: TX_HASH,
  type: 'staticcall',
  value: '22324344900000000',
};
