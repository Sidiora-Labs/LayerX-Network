import { WagmiAdapter } from '@reown/appkit-adapter-wagmi';
import type { AppKitNetwork } from '@reown/appkit/networks';
import type { Chain, Transport } from 'viem';
import { fallback, http } from 'viem';
import { createConfig } from 'wagmi';

import appConfig from 'configs/app';
import essentialDappsChainsConfig from 'configs/essential-dapps-chains';
import multichainConfig from 'configs/multichain';
import { chains, parentChain } from 'lib/web3/chains';

const feature = appConfig.features.blockchainInteraction;

const getChainTransportFromConfig = (config: Partial<typeof appConfig> | undefined, readOnly?: boolean): Record<string, Transport> => {
  if (!config?.chain?.id) {
    return {};
  }

  return {
    [config.chain.id]: fallback(
      config.chain.rpcUrls
        .concat(readOnly && config.apis?.general ? `${ config.apis.general.endpoint }${ config.apis.general.basePath ?? '' }/api/eth-rpc` : '')
        .filter(Boolean)
        .map((url) => http(url, { batch: { wait: 100, batchSize: 5 } })),
    ),
  };
};

const reduceExternalChainsToTransportConfig = (readOnly: boolean): Record<string, Transport> => {
  const multichain = multichainConfig();
  const essentialDapps = essentialDappsChainsConfig();
  const chains = [ ...(multichain?.chains ?? []), ...(essentialDapps?.chains ?? []) ].filter(Boolean);

  if (!chains) {
    return {};
  }

  return chains
    .map(({ app_config: config }) => getChainTransportFromConfig(config, readOnly))
    .reduce((result, item) => {
      Object.entries(item).map(([ id, transport ]) => {
        result[id] = transport;
      });
      return result;
    }, {} as Record<string, Transport>);
};

const wagmi = (() => {

  if (!feature.isEnabled || feature.connectorType === 'dynamic') {
    const wagmiConfig = createConfig({
      chains: chains as [Chain, ...Array<Chain>],
      transports: {
        ...getChainTransportFromConfig(appConfig, true),
        ...(parentChain ? { [parentChain.id]: http(parentChain.rpcUrls.default.http[0]) } : {}),
        ...reduceExternalChainsToTransportConfig(true),
      },
      ssr: true,
      batch: { multicall: { wait: 100, batchSize: 5 } },
      multiInjectedProviderDiscovery: feature.isEnabled && feature.connectorType === 'dynamic' ? false : true,
    });

    return { config: wagmiConfig, adapter: null };
  }

  const wagmiAdapter = new WagmiAdapter({
    networks: chains as Array<AppKitNetwork>,
    multiInjectedProviderDiscovery: true,
    transports: {
      ...getChainTransportFromConfig(appConfig, false),
      ...(parentChain ? { [parentChain.id]: http() } : {}),
      ...reduceExternalChainsToTransportConfig(false),
    },
    projectId: feature.reown.projectId,
    ssr: true,
    batch: { multicall: { wait: 100, batchSize: 5 } },
    syncConnectedChain: false,
  });

  return { config: wagmiAdapter.wagmiConfig, adapter: wagmiAdapter };
})();

export default wagmi;
