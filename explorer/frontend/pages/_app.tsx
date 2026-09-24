import type { HTMLChakraProps } from '@chakra-ui/react';
import { GrowthBookProvider } from '@growthbook/growthbook-react';
import { QueryClientProvider } from '@tanstack/react-query';
import { ReactQueryDevtools } from '@tanstack/react-query-devtools';
import type { AppProps } from 'next/app';
import dynamic from 'next/dynamic';
import React from 'react';

import type { NextPageWithLayout } from 'nextjs/types';

import type { Route } from 'nextjs-routes';
import PageMetadata from 'nextjs/PageMetadata';

import config from 'configs/app';
import getSocketUrl from 'lib/api/getSocketUrl';
import useQueryClientConfig from 'lib/api/useQueryClientConfig';
import { AppContextProvider } from 'lib/contexts/app';
import { FallbackProvider } from 'lib/contexts/fallback';
import { MarketplaceContextProvider } from 'lib/contexts/marketplace';
import { SettingsContextProvider } from 'lib/contexts/settings';
import { initGrowthBook } from 'lib/growthbook/init';
import useLoadFeatures from 'lib/growthbook/useLoadFeatures';
import { clientConfig as rollbarConfig, Provider as RollbarProvider } from 'lib/rollbar';
import { SocketProvider } from 'lib/socket/context';
import { Provider as ChakraProvider } from 'toolkit/chakra/provider';
import { Toaster } from 'toolkit/chakra/toaster';
import AppErrorBoundary from 'ui/shared/AppError/AppErrorBoundary';
import AppErrorGlobalContainer from 'ui/shared/AppError/AppErrorGlobalContainer';
import GoogleAnalytics from 'ui/shared/GoogleAnalytics';
import Layout from 'ui/shared/layout/Layout';
import Web3Provider from 'ui/shared/web3/Web3Provider';

const RewardsContextProvider = dynamic(() => import('lib/contexts/rewards').then(module => module.RewardsContextProvider), { ssr: false });
const RewardsLoginModal = dynamic(() => import('ui/rewards/login/RewardsLoginModal'), { ssr: false });
const RewardsActivityTracker = dynamic(() => import('ui/rewards/RewardsActivityTracker'), { ssr: false });

import 'lib/setLocale';
// import 'focus-visible/dist/focus-visible';
import 'nextjs/global.css';

type AppPropsWithLayout = AppProps & {
  Component: NextPageWithLayout;
};

const ERROR_SCREEN_STYLES: HTMLChakraProps<'div'> = {
  h: '100vh',
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'flex-start',
  justifyContent: 'center',
  width: 'fit-content',
  maxW: '800px',
  margin: { base: '0 auto', lg: '0 auto' },
  p: { base: 4, lg: 0 },
};

const CONSOLE_SCAM_WARNING = `⚠️WARNING: Do not paste or execute any scripts here!
Anyone asking you to run code here might be trying to scam you and steal your data.
If you don't understand what this console is for, close it now and stay safe.`;

const CONSOLE_SCAM_WARNING_DELAY_MS = 500;

function MyApp({ Component, pageProps, router }: AppPropsWithLayout) {

  const growthBook = initGrowthBook(pageProps.uuid);
  useLoadFeatures(growthBook);

  const queryClient = useQueryClientConfig();

  React.useEffect(() => {
    // after the app is rendered/hydrated, show the console scam warning
    const timeoutId = window.setTimeout(() => {
      // eslint-disable-next-line no-console
      console.warn(CONSOLE_SCAM_WARNING);
    }, CONSOLE_SCAM_WARNING_DELAY_MS);

    return () => window.clearTimeout(timeoutId);
  }, []);

  const content = (() => {
    const getLayout = Component.getLayout ?? ((page) => <Layout>{ page }</Layout>);

    return (
      <>
        { getLayout(<Component { ...pageProps }/>) }
        <Toaster/>
        { config.features.rewards.isEnabled && (
          <>
            <RewardsLoginModal/>
            <RewardsActivityTracker/>
          </>
        ) }
      </>
    );
  })();

  const RewardsProvider = config.features.rewards.isEnabled ? RewardsContextProvider : FallbackProvider;

  const socketUrl = !config.features.multichain.isEnabled ? getSocketUrl() : undefined;

  return (
    <>
      <PageMetadata pathname={ router.pathname as Route['pathname'] } query={ pageProps.query } apiData={ pageProps.apiData }/>
      <ChakraProvider>
        <RollbarProvider config={ rollbarConfig }>
          <AppErrorBoundary
            { ...ERROR_SCREEN_STYLES }
            Container={ AppErrorGlobalContainer }
          >
            <QueryClientProvider client={ queryClient }>
              <Web3Provider>
                <AppContextProvider pageProps={ pageProps }>
                  <GrowthBookProvider growthbook={ growthBook }>
                    <SocketProvider url={ socketUrl }>
                      <RewardsProvider>
                        <MarketplaceContextProvider>
                          <SettingsContextProvider>
                            { content }
                          </SettingsContextProvider>
                        </MarketplaceContextProvider>
                      </RewardsProvider>
                    </SocketProvider>
                  </GrowthBookProvider>
                  <ReactQueryDevtools buttonPosition="bottom-left" position="left"/>
                  <GoogleAnalytics/>
                </AppContextProvider>
              </Web3Provider>
            </QueryClientProvider>
          </AppErrorBoundary>
        </RollbarProvider>
      </ChakraProvider>
    </>
  );
}

export default MyApp;
