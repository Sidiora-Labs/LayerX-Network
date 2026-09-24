import type { Block } from 'types/api/block';

import { ADDRESS_PARAMS } from './addressParams';

export const BLOCK_HASH = '0x8fa7b9e5e5e79deeb62d608db22ba9a5cb45388c7ebb9223ae77331c6080dc70';

export const BLOCK: Block = {
  base_fee_per_gas: '10000000000',
  burnt_fees: '92834504000000000',
  burnt_fees_percentage: 42.2,
  difficulty: '340282366920938463463374607431768211451',
  extra_data: 'TODO',
  gas_limit: '30000000',
  gas_target_percentage: 55.79,
  gas_used: '6631036',
  gas_used_percentage: 22.10,
  hash: BLOCK_HASH,
  height: 8988736,
  miner: ADDRESS_PARAMS,
  nonce: '0x0000000000000000',
  parent_hash: BLOCK_HASH,
  priority_fee: '19241635454943109',
  rewards: [
    {
      reward: '19241635454943109',
      type: 'Validator Reward',
    },
  ],
  size: 46406,
  state_root: 'TODO',
  timestamp: '2023-05-12T19:29:12.000000Z',
  total_difficulty: '10837812015930321201107455268036056402048391639',
  transactions_count: 142,
  internal_transactions_count: 42,
  transaction_fees: '19241635547777613',
  type: 'block',
  uncles_hashes: [],
};
