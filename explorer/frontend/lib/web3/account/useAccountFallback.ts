import React from 'react';
import type { UseAccountReturnType } from 'wagmi';

export default function useAccountFallback(): UseAccountReturnType {
  return React.useMemo(() => ({
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
  }), []);
}
