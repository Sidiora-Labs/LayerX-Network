import { useRouter } from 'next/router';
import { useEffect, useRef } from 'react';

import removeQueryParam from 'lib/router/removeQueryParam';
import updateQueryParam from 'lib/router/updateQueryParam';
import useWeb3Wallet from 'lib/web3/useWallet';

export default function useAutoConnectWallet() {
  const router = useRouter();
  const web3Wallet = useWeb3Wallet({ source: 'Swap button' });
  const isConnectionStarted = useRef(false);

  useEffect(() => {
    if (router.query.action !== 'connect') {
      return;
    }

    let timer: ReturnType<typeof setTimeout>;

    if (!web3Wallet.isConnected && !web3Wallet.isOpen) {
      if (!isConnectionStarted.current) {
        timer = setTimeout(() => {
          if (!web3Wallet.isConnected) {
            web3Wallet.connect();
            isConnectionStarted.current = true;
          }
        }, 500);
      } else {
        isConnectionStarted.current = false;
        updateQueryParam(router, 'action', 'tooltip');
      }
    } else if (web3Wallet.isConnected) {
      isConnectionStarted.current = false;
      removeQueryParam(router, 'action');
    }

    return () => clearTimeout(timer);
  }, [ router, web3Wallet ]);
}
