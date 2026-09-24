import type { ExternalChain } from 'types/externalChains';

import type config from 'configs/app';

export interface ClusterChainConfig extends ExternalChain {
  slug: string;
  app_config: typeof config;
}

export interface MultichainConfig {
  chains: Array<ClusterChainConfig>;
}
