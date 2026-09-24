import { useDynamicContext, useDynamicEvents, useUserWallets } from '@dynamic-labs/sdk-react-core';
import React from 'react';
import { useAccountEffect } from 'wagmi';

import type { Params, Result } from './types';

import * as mixpanel from 'lib/mixpanel/index';

import useAccountDynamic from '../account/useAccountDynamic';

export default function useWalletDynamic({ source, onConnect }: Params): Result {
  const isConnectionStarted = React.useRef(false);
  const [ isOpen ] = React.useState(false);
  const [ isClientLoaded, setIsClientLoaded ] = React.useState(false);

  const { setShowDynamicUserProfile, setShowAuthFlow, setAuthMode, removeWallet, primaryWallet } = useDynamicContext();

  const openModal = React.useCallback(() => {
    setShowDynamicUserProfile(true);
    mixpanel.logEvent(mixpanel.EventTypes.ACCOUNT_ACCESS, { Action: 'Dropdown open' });
  }, [ setShowDynamicUserProfile ]);

  useDynamicEvents('authFlowOpen', async() => {
    if (!isConnectionStarted.current) {
      mixpanel.logEvent(mixpanel.EventTypes.WALLET_CONNECT, { Source: source, Status: 'Started' });
      isConnectionStarted.current = true;
    }
  });

  const handleAccountConnected = React.useCallback(({ isReconnected }: { isReconnected: boolean }) => {
    if (!isReconnected && isConnectionStarted.current) {
      mixpanel.logEvent(mixpanel.EventTypes.WALLET_CONNECT, { Source: source, Status: 'Connected' });
      mixpanel.userProfile.setOnce({
        'With Connected Wallet': true,
      });
      onConnect?.();
    }
    isConnectionStarted.current = false;
  }, [ source, onConnect ]);

  const handleConnect = React.useCallback(() => {
    setAuthMode('connect-only');
    setShowAuthFlow(true);
  }, [ setAuthMode, setShowAuthFlow ]);

  const userWallets = useUserWallets();
  const primaryWalletId = userWallets[0]?.id;

  const handleDisconnect = React.useCallback(() => {
    primaryWalletId && removeWallet(primaryWalletId);
  }, [ primaryWalletId, removeWallet ]);

  useAccountEffect({ onConnect: handleAccountConnected });

  React.useEffect(() => {
    setIsClientLoaded(true);
  }, []);

  const account = useAccountDynamic();
  const address = account.address;
  const isConnected = isClientLoaded && !account.isDisconnected && account.address !== undefined;
  const isDynamicWaaS = primaryWallet?.key === 'dynamicwaas';

  return React.useMemo(() => ({
    connect: handleConnect,
    disconnect: handleDisconnect,
    isOpen,
    isConnected,
    isReconnecting: false,
    address,
    openModal,
    type: isDynamicWaaS ? 'dynamicwaas' : undefined,
  }), [ handleConnect, handleDisconnect, isOpen, isConnected, address, openModal, isDynamicWaaS ]);
}
