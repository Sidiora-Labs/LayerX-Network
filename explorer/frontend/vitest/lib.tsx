import { GrowthBookProvider } from '@growthbook/growthbook-react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { RenderOptions } from '@testing-library/react';
import { render } from '@testing-library/react';
import React from 'react';

import { AppContextProvider } from 'lib/contexts/app';
import { SocketProvider } from 'lib/socket/context';

import 'lib/setLocale';

const PAGE_PROPS = {
  cookies: '',
  referrer: '',
  query: {},
  adBannerProvider: null,
  apiData: null,
  uuid: '123',
};

const TestApp = ({ children }: { children: React.ReactNode }) => {
  const [ queryClient ] = React.useState(() => new QueryClient({
    defaultOptions: {
      queries: {
        refetchOnWindowFocus: false,
        retry: 0,
      },
    },
  }));

  return (
    <QueryClientProvider client={ queryClient }>
      <AppContextProvider pageProps={ PAGE_PROPS }>
        <GrowthBookProvider>
          <SocketProvider>
            { children }
          </SocketProvider>
        </GrowthBookProvider>
      </AppContextProvider>
    </QueryClientProvider>
  );
};

const customRender = (
  ui: React.ReactElement,
  options?: Omit<RenderOptions, 'wrapper'>,
) => render(ui, { wrapper: TestApp, ...options });

export * from '@testing-library/react';
export { customRender as render };
export { TestApp as wrapper };
