import React from 'react';

import type { Result } from './types';

export default function useWalletFallback(): Result {
  return React.useMemo(() => ({
    connect: () => {},
    disconnect: () => {},
    isOpen: false,
    isConnected: false,
    isReconnecting: false,
    address: undefined,
    openModal: () => {},
  }), []);
}
