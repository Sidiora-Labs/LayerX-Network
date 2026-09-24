import type { ClusterByNameResponse, ClusterByIdResponse } from 'types/api/clusters';

export const campNetworkClusterByName: ClusterByNameResponse = {
  result: {
    data: {
      name: 'campnetwork/lol',
      owner: '0x1234567890123456789012345678901234567890',
      clusterId: 'clstr_1a2b3c4d5e6f7g8h9i0j',
      backingWei: '5000000000000000000',
      expiresAt: '2025-01-15T10:30:00Z',
      createdAt: '2024-01-15T10:30:00Z',
      updatedAt: '2024-01-20T14:22:00Z',
      updatedBy: '0x1234567890123456789012345678901234567890',
      isTestnet: false,
    },
  },
};

export const duckClusterByName: ClusterByNameResponse = {
  result: {
    data: {
      name: 'duck/quack',
      owner: '0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef',
      clusterId: 'clstr_9z8y7x6w5v4u3t2s1r0q',
      backingWei: '12000000000000000000',
      expiresAt: null,
      createdAt: '2024-02-01T08:15:00Z',
      updatedAt: '2024-02-05T16:45:00Z',
      updatedBy: '0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef',
      isTestnet: false,
    },
  },
};

export const campNetworkClusterById: ClusterByIdResponse = {
  result: {
    data: {
      id: 'clstr_1a2b3c4d5e6f7g8h9i0j',
      createdBy: '0x1234567890123456789012345678901234567890',
      createdAt: '2024-01-15T10:30:00Z',
      wallets: [
        {
          address: '0x1234567890123456789012345678901234567890',
          name: 'main.campnetwork',
          chainIds: [ '1', '137' ],
        },
        {
          address: '0x0987654321098765432109876543210987654321',
          name: 'treasury.campnetwork',
          chainIds: [ '1' ],
        },
        {
          address: '0x1111222233334444555566667777888899990000',
          name: 'staking.campnetwork',
          chainIds: [ '137', '56' ],
        },
      ],
      isTestnet: false,
    },
  },
};

export const testnetClusterByName: ClusterByNameResponse = {
  result: {
    data: {
      name: 'test/cluster',
      owner: '0x9876543210987654321098765432109876543210',
      clusterId: 'clstr_test123456789',
      backingWei: '1000000000000000000',
      expiresAt: '2024-12-31T23:59:59Z',
      createdAt: '2024-03-01T12:00:00Z',
      updatedAt: '2024-03-01T12:00:00Z',
      updatedBy: '0x9876543210987654321098765432109876543210',
      isTestnet: true,
    },
  },
};
