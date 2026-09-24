import type { ExternalChain } from 'types/externalChains';

import type { Route } from 'nextjs-routes';
import { route as nextjsRoute } from 'nextjs-routes';

import { stripTrailingSlash } from 'toolkit/utils/url';

export interface RouteParams {
  external?: boolean;
  chain?: ExternalChain & { slug?: string };
}

export const route = (route: Route, params?: RouteParams | null) => {
  const generatedRoute = nextjsRoute(routeParams(route, params));

  if (params && params.chain && params.external && params.chain.explorer_url) {
    return stripTrailingSlash(params.chain.explorer_url) + generatedRoute;
  }

  return generatedRoute;
};

export const routeParams = (route: Route, params?: RouteParams | null): Route => {
  if (!params?.external && params?.chain?.slug) {
    const pathname = '/chain/[chain_slug]' + route.pathname;
    return { ...route, pathname, query: { ...route.query, chain_slug: params.chain.slug } } as Route;
  }
  return route;
};
