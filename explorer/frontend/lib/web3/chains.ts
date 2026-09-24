import type { Chain } from 'viem';

import appConfig from 'configs/app';
import essentialDappsChainsConfig from 'configs/essential-dapps-chains';
import multichainConfig from 'configs/multichain';

const getChainInfo = (
  config: Partial<typeof appConfig> = appConfig,
  contracts?: Chain['contracts'],
  logoUrl?: string,
): Chain | undefined => {
  if (!config.chain || !config.app) {
    return;
  }

  return {
    id: Number(config.chain.id),
    name: config.chain.name ?? '',
    nativeCurrency: {
      decimals: config.chain.currency.decimals,
      name: config.chain.currency.name ?? '',
      symbol: config.chain.currency.symbol ?? '',
    },
    rpcUrls: {
      'default': {
        http: config.chain.rpcUrls,
      },
    },
    blockExplorers: {
      'default': {
        name: 'Blockscout',
        url: config.app.baseUrl,
      },
    },
    testnet: config.chain.isTestnet,
    contracts,
    custom: {
      logoUrl: logoUrl ?? config.UI?.navigation.icon.default,
    },
  };
};

export const currentChain: Chain | undefined = !appConfig.features.multichain.isEnabled ? getChainInfo() : undefined;

export const parentChain: Chain | undefined = (() => {
  const rollupFeature = appConfig.features.rollup;

  const parentChain = rollupFeature.isEnabled && rollupFeature.parentChain;

  if (!parentChain) {
    return;
  }

  if (!parentChain.id || !parentChain.name || !parentChain.rpcUrls || !parentChain.baseUrl || !parentChain.currency) {
    return;
  }

  return {
    id: parentChain.id,
    name: parentChain.name,
    nativeCurrency: parentChain.currency,
    rpcUrls: {
      'default': {
        http: parentChain.rpcUrls,
      },
    },
    blockExplorers: {
      'default': {
        name: 'Blockscout',
        url: parentChain.baseUrl,
      },
    },
    testnet: parentChain.isTestnet,
  };
})();

export const clusterChains: Array<Chain> | undefined = (() => {
  const config = multichainConfig();

  if (!config) {
    return;
  }

  return config.chains.map(({ app_config: config, logo }) => getChainInfo(config, undefined, logo)).filter(Boolean);
})();

export const essentialDappsChains: Array<Chain> | undefined = (() => {
  const config = essentialDappsChainsConfig();

  if (!config) {
    return;
  }

  return config.chains.map(({ app_config: config, contracts, logo }) => getChainInfo(config, contracts, logo)).filter(Boolean);
})();

export const chains = (() => {
  if (essentialDappsChains) {
    const hasCurrentChain = essentialDappsChains.some((chain) => chain.id === currentChain?.id);
    const hasParentChain = essentialDappsChains.some((chain) => chain.id === parentChain?.id);

    return [
      ...essentialDappsChains,
      hasCurrentChain ? undefined : currentChain,
      hasParentChain ? undefined : parentChain,
    ].filter(Boolean);
  }

  return [ currentChain, parentChain, ...(clusterChains ?? []) ].filter(Boolean);
})();
