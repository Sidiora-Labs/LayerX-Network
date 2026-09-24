import type { InteropMessage } from 'types/api/interop';

import { ADDRESS_HASH } from './addressParams';
import { TX_HASH } from './tx';

export const INTEROP_MESSAGE: InteropMessage = {
  init_transaction_hash: TX_HASH,
  nonce: 52,
  payload: '0x4f0edcc90000000000000000000000007da521cbbe62e89cd75e0993c78b8c68c25f696b',
  relay_chain: {
    chain_id: 420120000,
    chain_name: 'Optimism Testnet',
    chain_logo: null,
    instance_url: 'https://optimism-interop-alpha-0.blockscout.com/',
  },
  relay_transaction_hash: TX_HASH,
  sender_address_hash: ADDRESS_HASH,
  status: 'Relayed',
  target_address_hash: ADDRESS_HASH,
  timestamp: '2025-02-20T01:05:14.000000Z',
};
