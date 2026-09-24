import type { ClustersLeaderboardObject } from 'types/api/clusters';

export const CLUSTER_ITEM: ClustersLeaderboardObject = {
  name: 'example.cluster',
  clusterId: '0x1234567890123456789012345678901234567890123456789012345678901234',
  isTestnet: false,
  totalWeiAmount: '1000000000000000000',
  totalReferralAmount: '100000000000000000',
  chainIds: [ '1', '137', '42161' ],
  nameCount: '10',
  rank: 1,
};
