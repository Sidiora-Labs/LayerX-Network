import { useDynamicContext, useUserWallets } from '@dynamic-labs/sdk-react-core';
import React from 'react';
import type { UseAccountReturnType } from 'wagmi';

export default function useAccountDynamic(): UseAccountReturnType {
  const userWallets = useUserWallets();
  const { primaryWallet } = useDynamicContext();

  const address = (primaryWallet?.address || userWallets[0]?.address) as `0x${ string }` | undefined;

  return React.useMemo(() => {
    if (!address) {
      return {
        address: undefined,
        addresses: undefined,
        chain: undefined,
        chainId: undefined,
        connector: undefined,
        isConnected: false,
        isConnecting: false,
        isDisconnected: true,
        isReconnecting: false,
        status: 'disconnected',
      };
    }

    return {
      address,
      addresses: [ address ],
      chain: undefined,
      chainId: undefined,
      connector: undefined,
      isConnected: true,
      isConnecting: false,
      isDisconnected: false,
      isReconnecting: false,
      status: 'connected',
    } as unknown as UseAccountReturnType;
  }, [ address ]);
}
