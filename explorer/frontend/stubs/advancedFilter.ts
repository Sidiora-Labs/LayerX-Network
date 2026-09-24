import type { AdvancedFilterResponseItem } from 'types/api/advancedFilter';

import { ADDRESS_PARAMS } from './addressParams';
import { TX_HASH } from './tx';

export const ADVANCED_FILTER_ITEM: AdvancedFilterResponseItem = {
  fee: '215504444616317',
  from: ADDRESS_PARAMS,
  hash: TX_HASH,
  method: 'approve',
  timestamp: '2022-11-11T11:11:11.000000Z',
  to: ADDRESS_PARAMS,
  token: null,
  total: null,
  type: 'coin_transfer',
  value: '42000420000000000000',
};
