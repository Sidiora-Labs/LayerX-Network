import { useRouter } from 'next/router';
import React, { createContext, useContext, useEffect, useState, useMemo } from 'react';

type Props = {
  children: React.ReactNode;
};

type TMarketplaceContext = {
  isAutoConnectDisabled: boolean;
  setIsAutoConnectDisabled: (isAutoConnectDisabled: boolean) => void;
};

export const MarketplaceContext = createContext<TMarketplaceContext>({
  isAutoConnectDisabled: false,
  setIsAutoConnectDisabled: () => {},
});

export function MarketplaceContextProvider({ children }: Props) {
  const router = useRouter();
  const [ isAutoConnectDisabled, setIsAutoConnectDisabled ] = useState(false);

  useEffect(() => {
    const handleRouteChange = () => {
      setIsAutoConnectDisabled(false);
    };

    router.events.on('routeChangeStart', handleRouteChange);

    return () => {
      router.events.off('routeChangeStart', handleRouteChange);
    };
  }, [ router.events ]);

  const value = useMemo(() => ({
    isAutoConnectDisabled,
    setIsAutoConnectDisabled,
  }), [ isAutoConnectDisabled, setIsAutoConnectDisabled ]);

  return (
    <MarketplaceContext.Provider value={ value }>
      { children }
    </MarketplaceContext.Provider>
  );
}

export function useMarketplaceContext() {
  return useContext(MarketplaceContext);
}
