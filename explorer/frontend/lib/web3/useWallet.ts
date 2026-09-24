import config from 'configs/app';

const feature = config.features.blockchainInteraction;

// eslint-disable-next-line no-nested-ternary
const useWallet = (feature.isEnabled && feature.connectorType === 'dynamic') ?
  (await import('./wallet/useWalletDynamic')).default :
  (feature.isEnabled && feature.connectorType === 'reown') ?
    (await import('./wallet/useWalletReown')).default :
    (await import('./wallet/useWalletFallback')).default;

export default useWallet;
