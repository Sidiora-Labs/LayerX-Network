import type { ClustersDirectoryResponse, ClustersDirectoryObject } from 'types/api/clusters';

export const campNetworkCluster: ClustersDirectoryObject = {
  name: 'campnetwork/lol',
  isTestnet: false,
  createdAt: '2024-01-15T10:30:00Z',
  owner: '0x1234567890123456789012345678901234567890',
  totalWeiAmount: '5000000000000000000',
  updatedAt: '2024-01-20T14:22:00Z',
  updatedBy: '0x1234567890123456789012345678901234567890',
  chainIds: [ '1', '137', '56' ],
};

export const duckCluster: ClustersDirectoryObject = {
  name: 'duck/quack',
  isTestnet: false,
  createdAt: '2024-02-01T08:15:00Z',
  owner: '0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef',
  totalWeiAmount: '12000000000000000000',
  updatedAt: '2024-02-05T16:45:00Z',
  updatedBy: '0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef',
  chainIds: [ '1', '42161' ],
};

export const testnetCluster: ClustersDirectoryObject = {
  name: 'test/cluster',
  isTestnet: true,
  createdAt: '2024-03-01T12:00:00Z',
  owner: '0x9876543210987654321098765432109876543210',
  totalWeiAmount: '1000000000000000000',
  updatedAt: '2024-03-01T12:00:00Z',
  updatedBy: '0x9876543210987654321098765432109876543210',
  chainIds: [ '11155111' ],
};

export const longNameCluster: ClustersDirectoryObject = {
  name: 'this-is-a-very-long-cluster-name-that-should-test-truncation/subdomain',
  isTestnet: false,
  createdAt: '2024-01-10T09:20:00Z',
  owner: '0x1111222233334444555566667777888899990000',
  totalWeiAmount: '750000000000000000',
  updatedAt: '2024-01-25T11:30:00Z',
  updatedBy: '0x1111222233334444555566667777888899990000',
  chainIds: [ '1' ],
};

export const clustersDirectoryMock: ClustersDirectoryResponse = {
  result: {
    data: {
      total: 4,
      items: [
        campNetworkCluster,
        duckCluster,
        testnetCluster,
        longNameCluster,
      ],
    },
  },
};

export const clustersDirectoryEmptyMock: ClustersDirectoryResponse = {
  result: {
    data: {
      total: 0,
      items: [],
    },
  },
};

export const clustersDirectoryLoadingMock: ClustersDirectoryResponse = {
  result: {
    data: {
      total: 0,
      items: [],
    },
  },
};
