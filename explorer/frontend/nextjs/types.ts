import type { NextPage } from 'next';
import type React from 'react';

import type { Route } from 'nextjs-routes';

export type NextPageWithLayout<P = {}, IP = P> = NextPage<P, IP> & {
  getLayout?: (page: React.ReactElement) => React.ReactNode;
};

export interface RouteParams<Pathname extends Route['pathname']> {
  pathname: Pathname;
  query?: Route['query'];
}
