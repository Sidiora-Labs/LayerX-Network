import type { EssentialDappsChainConfig } from 'types/client/marketplace';

import config from 'configs/app';
import { isBrowser } from 'toolkit/utils/isBrowser';

const marketplaceFeature = config.features.marketplace;

const essentialDappsChains: () => { chains: Array<EssentialDappsChainConfig> } | undefined = () => {
  if (!marketplaceFeature.isEnabled || !marketplaceFeature.essentialDapps) {
    return;
  }

  if (isBrowser()) {
    return window.__essentialDappsChains;
  }
};

export default essentialDappsChains;
