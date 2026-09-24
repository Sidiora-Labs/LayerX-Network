import dynamic from 'next/dynamic';

import config from 'configs/app';

const ReownProvider = dynamic(() => import('./providers/ReownProvider'), { ssr: false });
const DynamicProvider = dynamic(() => import('./providers/DynamicProvider'), { ssr: false });
const WagmiProvider = dynamic(() => import('./providers/WagmiProvider'), { ssr: false });

const feature = config.features.blockchainInteraction;

const Web3Provider = (() => {
  if (feature.isEnabled && feature.connectorType === 'reown') {
    return ReownProvider;
  }

  if (feature.isEnabled && feature.connectorType === 'dynamic') {
    return DynamicProvider;
  }

  return WagmiProvider;
})();

export default Web3Provider;
