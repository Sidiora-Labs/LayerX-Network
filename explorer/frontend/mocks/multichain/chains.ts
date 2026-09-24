import type { ClusterChainConfig } from 'types/multichain';

export const chainA = {
  slug: 'op-mainnet',
  name: 'OP Mainnet',
  id: '420',
  logo: 'https://example.com/logo_s.png',
  explorer_url: 'https://op-mainnet.com',
  app_config: {
    app: {
      baseUrl: 'https://op-mainnet.com',
    },
    chain: {
      currency: {
        name: 'Ether',
        symbol: 'ETH',
        decimals: 18,
      },
      rpcUrls: [
        'https://rpc.op-mainnet.com',
      ],
      additionalTokenTypes: [ ],
    },
    apis: {
      general: {
        host: 'localhost',
        protocol: 'http',
        port: '4003',
        endpoint: 'http://localhost:4003',
        socketEndpoint: 'ws://localhost:3200',
      },
    },
    UI: {
      views: {
        address: {},
      },
      navigation: {
        icon: {},
      },
    },
    features: {
      csvExport: {
        isEnabled: true,
        reCaptcha: { siteKey: 'xxx' },
      },
      advancedFilter: {
        isEnabled: true,
      },
      web3Wallet: {
        isEnabled: true,
        wallets: [ 'metamask' ],
      },
      rollup: {
        isEnabled: true,
        type: 'optimistic',
        stageIndex: '1',
        parentChain: {
          baseUrl: 'https://eth-mainnet.com',
        },
      },
    },
  },
} as unknown as ClusterChainConfig;

export const chainB = {
  ...chainA,
  id: '421',
  name: 'OP Testnet',
  logo: 'https://example.com/logo_md.png',
  slug: 'op-testnet',
  explorer_url: 'https://op-testnet.com',
  app_config: {
    ...chainA.app_config,
    chain: {
      ...chainA.app_config?.chain,
      isTestnet: true,
    },
    app: {
      baseUrl: 'https://op-testnet.com',
    },
    apis: {
      general: {
        ...chainA.app_config?.apis?.general,
        port: '4004',
        endpoint: 'http://localhost:4004',
      },
    },
  },
} as ClusterChainConfig;

export const chainC = {
  ...chainA,
  id: '422',
  name: 'OP Devnet',
  slug: 'op-devnet',
  explorer_url: 'https://op-devnet.com',
  logo: undefined,
  app_config: {
    ...chainA.app_config,
    chain: {
      ...chainA.app_config?.chain,
      isTestnet: true,
    },
    app: {
      baseUrl: 'https://op-devnet.com',
    },
    apis: {
      general: {
        ...chainA.app_config?.apis?.general,
        port: '4005',
        endpoint: 'http://localhost:4005',
      },
    },
  },
} as ClusterChainConfig;

export const chainD = {
  ...chainC,
  id: '423',
  name: 'Yellow rubber duck quack-quack-quack chain',
  slug: 'yellow-rubber-duck-chain',
};

export const chainE = {
  ...chainC,
  id: '424',
  name: 'White goose',
  slug: 'white-goose',
};
