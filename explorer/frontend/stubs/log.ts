import type { Log } from 'types/api/log';

import { ADDRESS_PARAMS } from './addressParams';
import { TX_HASH } from './tx';

export const LOG: Log = {
  address: ADDRESS_PARAMS,
  data: '0x000000000000000000000000000000000000000000000000000000d75e4be200',
  decoded: {
    method_call: 'CreditSpended(uint256 indexed _type, uint256 _quantity)',
    method_id: '58cdf94a',
    parameters: [
      {
        indexed: true,
        name: '_type',
        type: 'uint256',
        value: 'placeholder',
      },
      {
        indexed: false,
        name: '_quantity',
        type: 'uint256',
        value: 'placeholder',
      },
    ],
  },
  index: 42,
  topics: [
    '0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925',
    '0x000000000000000000000000c52ea157a7fb3e25a069d47df0428ac70cd656b1',
    '0x000000000000000000000000302fd86163cb9ad5533b3952dafa3b633a82bc51',
    null,
  ],
  transaction_hash: TX_HASH,
  block_timestamp: '2022-02-02T12:00:00Z',
};
