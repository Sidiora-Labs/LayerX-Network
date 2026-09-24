import React, { createContext, useContext } from 'react';

import type { Route } from 'nextjs-routes';
import type { Props as PageProps } from 'nextjs/getServerSideProps/handlers';

type Props = {
  children: React.ReactNode;
  pageProps: PageProps;
};

const AppContext = createContext<PageProps>({
  cookies: '',
  referrer: '',
  query: {},
  adBannerProvider: null,
  apiData: null,
  uuid: '',
});

export function AppContextProvider({ children, pageProps }: Props) {
  return (
    <AppContext.Provider value={ pageProps }>
      { children }
    </AppContext.Provider>
  );
}

export function useAppContext<Pathname extends Route['pathname'] = never>() {
  return useContext<PageProps<Pathname>>(AppContext);
}
