import type { CeloEpochListItem, CeloEpochDetails, CeloEpochElectionReward } from 'types/api/epochs';

import { BLOCK_HASH } from './block';
import { TOKEN_INFO_ERC_20, TOKEN_TRANSFER_ERC_20, TOKEN_TRANSFER_ERC_20_TOTAL } from './token';

export const CELO_EPOCH_ITEM: CeloEpochListItem = {
  timestamp: '2025-06-10T01:27:52.000000Z',
  number: 1739,
  end_block_number: 48563551,
  start_block_number: 48477132,
  type: 'L1',
  is_finalized: true,
  distribution: {
    carbon_offsetting_transfer: TOKEN_TRANSFER_ERC_20_TOTAL,
    community_transfer: TOKEN_TRANSFER_ERC_20_TOTAL,
    transfers_total: TOKEN_TRANSFER_ERC_20_TOTAL,
  },
};

const CELO_EPOCH_REWARD: CeloEpochElectionReward = {
  count: 10,
  total: '157705500305820107521',
  token: TOKEN_INFO_ERC_20,
};

export const CELO_EPOCH: CeloEpochDetails = {
  timestamp: '2025-06-10T01:27:52.000000Z',
  number: 1739,
  start_block_number: 48477132,
  start_processing_block_hash: BLOCK_HASH,
  start_processing_block_number: 48563546,
  end_processing_block_hash: BLOCK_HASH,
  end_processing_block_number: 48563552,
  end_block_number: 48563551,
  type: 'L1',
  is_finalized: true,
  distribution: {
    carbon_offsetting_transfer: TOKEN_TRANSFER_ERC_20,
    community_transfer: TOKEN_TRANSFER_ERC_20,
    transfers_total: {
      token: TOKEN_INFO_ERC_20,
      total: TOKEN_TRANSFER_ERC_20_TOTAL,
    },
  },
  aggregated_election_rewards: {
    group: CELO_EPOCH_REWARD,
    validator: CELO_EPOCH_REWARD,
    voter: CELO_EPOCH_REWARD,
    delegated_payment: CELO_EPOCH_REWARD,
  },
};
