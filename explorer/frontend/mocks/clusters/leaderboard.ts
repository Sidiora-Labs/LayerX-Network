import type { ClustersLeaderboardResponse, ClustersLeaderboardObject } from 'types/api/clusters';

export const leaderboardFirst: ClustersLeaderboardObject = {
  name: 'ethereum/foundation',
  clusterId: 'clstr_eth_foundation_001',
  isTestnet: false,
  totalWeiAmount: '50000000000000000000',
  totalReferralAmount: '5000000000000000000',
  chainIds: [ '1', '137', '56', '42161' ],
  nameCount: '127',
  rank: 1,
};

export const leaderboardSecond: ClustersLeaderboardObject = {
  name: 'campnetwork/lol',
  clusterId: 'clstr_1a2b3c4d5e6f7g8h9i0j',
  isTestnet: false,
  totalWeiAmount: '25000000000000000000',
  totalReferralAmount: '2500000000000000000',
  chainIds: [ '1', '137', '56' ],
  nameCount: '89',
  rank: 2,
};

export const leaderboardThird: ClustersLeaderboardObject = {
  name: 'duck/quack',
  clusterId: 'clstr_9z8y7x6w5v4u3t2s1r0q',
  isTestnet: false,
  totalWeiAmount: '18000000000000000000',
  totalReferralAmount: '1800000000000000000',
  chainIds: [ '1', '42161' ],
  nameCount: '56',
  rank: 3,
};

export const leaderboardFourth: ClustersLeaderboardObject = {
  name: 'defi/protocol',
  clusterId: 'clstr_defi_protocol_xyz',
  isTestnet: false,
  totalWeiAmount: '12000000000000000000',
  totalReferralAmount: '1200000000000000000',
  chainIds: [ '1' ],
  nameCount: '34',
  rank: 4,
};

export const leaderboardFifth: ClustersLeaderboardObject = {
  name: 'gaming/world',
  clusterId: 'clstr_gaming_world_abc',
  isTestnet: false,
  totalWeiAmount: '8000000000000000000',
  totalReferralAmount: '800000000000000000',
  chainIds: [ '137', '56' ],
  nameCount: '23',
  rank: 5,
};

export const clustersLeaderboardMock: ClustersLeaderboardResponse = {
  result: {
    data: [
      leaderboardFirst,
      leaderboardSecond,
      leaderboardThird,
      leaderboardFourth,
      leaderboardFifth,
    ],
  },
};

export const clustersLeaderboardEmptyMock: ClustersLeaderboardResponse = {
  result: {
    data: [],
  },
};
