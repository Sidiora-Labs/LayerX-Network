import { GrowthBookProvider } from '@growthbook/growthbook-react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import React from 'react';
import { http } from 'viem';
import { WagmiProvider, createConfig } from 'wagmi';
import { mainnet } from 'wagmi/chains';
import { mock } from 'wagmi/connectors';

import type { Props as PageProps } from 'nextjs/getServerSideProps/handlers';

import config from 'configs/app';
import { AppContextProvider } from 'lib/contexts/app';
import { MarketplaceContext } from 'lib/contexts/marketplace';
import { RewardsContextProvider } from 'lib/contexts/rewards';
import { SettingsContextProvider } from 'lib/contexts/settings';
import { SocketProvider } from 'lib/socket/context';
import { currentChain } from 'lib/web3/chains';
import { Provider as ChakraProvider } from 'toolkit/chakra/provider';

import { port as socketPort } from './utils/socket';

export type Props = {
  children: React.ReactNode;
  withSocket?: boolean;
  withWalletClient?: boolean;
  appContext?: {
    pageProps: PageProps;
  };
  marketplaceContext?: {
    isAutoConnectDisabled: boolean;
    setIsAutoConnectDisabled: (isAutoConnectDisabled: boolean) => void;
  };
};

const defaultAppContext = {
  pageProps: {
    cookies: '',
    referrer: '',
    query: {},
    adBannerProvider: 'slise' as const,
    apiData: null,
    uuid: '123',
  },
};

const defaultMarketplaceContext = {
  isAutoConnectDisabled: false,
  setIsAutoConnectDisabled: () => {},
};

const wagmiConfig = createConfig({
  chains: [ currentChain ?? mainnet ],
  connectors: [
    mock({
      accounts: [
        '0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266',
      ],
    }),
  ],
  transports: {
    [currentChain?.id ?? mainnet.id]: http(),
  },
});

const TestApp = ({ children, withSocket, appContext = defaultAppContext, marketplaceContext = defaultMarketplaceContext }: Props) => {
  const [ queryClient ] = React.useState(() => new QueryClient({
    defaultOptions: {
      queries: {
        refetchOnWindowFocus: false,
        retry: 0,
      },
    },
  }));

  return (
    <ChakraProvider>
      <QueryClientProvider client={ queryClient }>
        <SocketProvider url={ withSocket ? `ws://${ config.app.host }:${ socketPort }` : undefined }>
          <AppContextProvider { ...appContext }>
            <MarketplaceContext.Provider value={ marketplaceContext }>
              <SettingsContextProvider>
                <GrowthBookProvider>
                  <WagmiProvider config={ wagmiConfig! }>
                    <RewardsContextProvider>
                      { children }
                    </RewardsContextProvider>
                  </WagmiProvider>
                </GrowthBookProvider>
              </SettingsContextProvider>
            </MarketplaceContext.Provider>
          </AppContextProvider>
        </SocketProvider>
      </QueryClientProvider>
    </ChakraProvider>
  );
};

export default TestApp;
