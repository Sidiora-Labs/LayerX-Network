// https://eips.ethereum.org/EIPS/eip-6963

import React from 'react';

import type { WalletType } from 'types/client/wallets';
import type { WalletProvider } from 'types/web3';

interface EIP6963ProviderInfo {
  uuid: string;
  name: string;
  icon: string;
  rdns: string;
}

interface EIP6963ProviderDetail {
  info: EIP6963ProviderInfo;
  provider: WalletProvider;
}

const WALLET_RDNS_MAP: Record<WalletType, string> = {
  metamask: 'io.metamask',
  coinbase: 'com.coinbase.wallet',
  token_pocket: 'io.tokenpocket',
  rabby: 'io.rabby',
  okx: 'com.okex.wallet',
  trust: 'com.trustwallet.app',
} as const;

const DETECTED_PROVIDERS: Partial<Record<WalletType, WalletProvider | false>> = {};

export default function useDetectWalletEip6963() {
  const detectionTimeoutRef = React.useRef<number | null>(null);

  const handleAnnounceProviderEvent = React.useCallback((event: CustomEvent<EIP6963ProviderDetail>) => {
    const wallet = Object.entries(WALLET_RDNS_MAP)
      .find(([ , rdns ]) => rdns === event.detail.info.rdns)?.[0] as WalletType | undefined;

    if (wallet && !DETECTED_PROVIDERS[wallet]) {
      DETECTED_PROVIDERS[wallet] = event.detail?.provider;
    }
  }, []);

  const detect = React.useCallback((wallet: WalletType) => {
    return new Promise<{ wallet: WalletType; provider: WalletProvider } | undefined>((resolve) => {
      const provider = DETECTED_PROVIDERS[wallet];

      if (provider) {
        resolve({ wallet, provider });
        return;
      }

      window.addEventListener('eip6963:announceProvider', handleAnnounceProviderEvent as EventListener);
      window.dispatchEvent(new Event('eip6963:requestProvider'));

      detectionTimeoutRef.current = window.setTimeout(() => {
        const provider = DETECTED_PROVIDERS[wallet];
        resolve(provider ? { wallet, provider } : undefined);
      }, 200);
    });
  }, [ handleAnnounceProviderEvent ]);

  React.useEffect(() => {
    return () => {
      window.removeEventListener('eip6963:announceProvider', handleAnnounceProviderEvent as EventListener);
    };
  }, [ handleAnnounceProviderEvent ]);

  return React.useMemo(() => ({
    detect,
  }), [ detect ]);
}
