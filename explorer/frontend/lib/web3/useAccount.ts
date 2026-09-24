import config from 'configs/app';

const feature = config.features.blockchainInteraction;

// eslint-disable-next-line no-nested-ternary
const useAccount = (feature.isEnabled && feature.connectorType === 'dynamic') ?
  (await import('./account/useAccountDynamic')).default :
  (feature.isEnabled && feature.connectorType === 'reown') ?
    (await import('wagmi')).useAccount :
    (await import('./account/useAccountFallback')).default;

export default useAccount;
