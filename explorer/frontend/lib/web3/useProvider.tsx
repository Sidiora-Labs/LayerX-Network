import { useQuery } from '@tanstack/react-query';

import { getFeaturePayload } from 'configs/app/features/types';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';

import detectWallet from './detectWallet';
import useDetectWalletEip6963 from './useDetectWalletEip6963';

export default function useProvider() {
  const multichainContext = useMultichainContext();
  const feature = (multichainContext?.chain.app_config ?? config).features.web3Wallet;
  const wallets = getFeaturePayload(feature)?.wallets;

  const { detect: detectWalletEip6963 } = useDetectWalletEip6963();

  return useQuery({
    queryKey: [ 'web3-wallet' ],
    queryFn: async() => {
      if (!feature.isEnabled || !wallets) {
        return;
      }

      if (!('ethereum' in window && window.ethereum)) {
        if (wallets.includes('metamask') && window.navigator.userAgent.includes('Firefox')) {
          const { WindowPostMessageStream } = (await import('@metamask/post-message-stream'));
          const { initializeProvider } = (await import('@metamask/providers'));

          // workaround for MetaMask in Firefox
          // Firefox blocks MetaMask injection script because of our CSP for 'script-src'
          // so we have to inject it manually while the issue is not fixed
          // https://github.com/MetaMask/metamask-extension/issues/3133#issuecomment-1025641185
          const metamaskStream = new WindowPostMessageStream({
            name: 'metamask-inpage',
            target: 'metamask-contentscript',
          });

          // this will initialize the provider and set it as window.ethereum
          initializeProvider({
            connectionStream: metamaskStream as never,
            shouldShimWeb3: true,
          });
        } else {
          return;
        }
      }

      // have to check again in case provider was not set as window.ethereum in the previous step for MM in FF
      // and also it makes typescript happy
      if (!('ethereum' in window && window.ethereum)) {
        return;
      }

      for (const wallet of wallets) {
        const detectedWalletEip6963 = await detectWalletEip6963(wallet);

        if (detectedWalletEip6963) {
          return detectedWalletEip6963;
        }

        const detectedWallet = detectWallet(wallet);

        if (detectedWallet) {
          return detectedWallet;
        }
      }
    },
    enabled: Boolean(feature.isEnabled || !wallets),
    refetchOnMount: false,
    staleTime: Infinity,
  });
}
