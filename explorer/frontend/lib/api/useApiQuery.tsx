import type { UseQueryOptions } from '@tanstack/react-query';
import { useQuery } from '@tanstack/react-query';

import type { ExternalChainExtended } from 'types/externalChains';

import { useMultichainContext } from 'lib/contexts/multichain';
import type { Params as FetchParams } from 'lib/hooks/useFetch';

import type { ResourceError, ResourceName, ResourcePathParams, ResourcePayload } from './resources';
import useApiFetch from './useApiFetch';

export interface Params<R extends ResourceName, E = unknown, D = ResourcePayload<R>> {
  pathParams?: ResourcePathParams<R>;
  queryParams?: Record<string, string | Array<string> | number | boolean | undefined | null>;
  fetchParams?: Pick<FetchParams, 'body' | 'method' | 'headers'>;
  queryOptions?: Partial<Omit<UseQueryOptions<ResourcePayload<R>, ResourceError<E>, D>, 'queryFn'>>;
  logError?: boolean;
  chain?: ExternalChainExtended;
}

export interface GetResourceKeyParams<R extends ResourceName, E = unknown, D = ResourcePayload<R>>
  extends Pick<Params<R, E, D>, 'pathParams' | 'queryParams'> {
  chainId?: string;
}

export function getResourceKey<R extends ResourceName>(resource: R, { pathParams, queryParams, chainId }: GetResourceKeyParams<R> = {}) {
  if (pathParams || queryParams) {
    return [ resource, chainId, { ...pathParams, ...queryParams } ].filter(Boolean);
  }

  return [ resource, chainId ].filter(Boolean);
}

export default function useApiQuery<R extends ResourceName, E = unknown, D = ResourcePayload<R>>(
  resource: R,
  { queryOptions, pathParams, queryParams, fetchParams, logError, chain: chainProp }: Params<R, E, D> = {},
) {
  const apiFetch = useApiFetch();
  const multichainContext = useMultichainContext();
  const chain = chainProp || multichainContext?.chain;

  return useQuery<ResourcePayload<R>, ResourceError<E>, D>({
    queryKey: queryOptions?.queryKey || getResourceKey(resource, { pathParams, queryParams, chainId: chain?.id }),
    queryFn: async({ signal }) => {
      // all errors and error typing is handled by react-query
      // so error response will never go to the data
      // that's why we are safe here to do type conversion "as Promise<ResourcePayload<R>>"
      return apiFetch(resource, { pathParams, queryParams, chain, logError, fetchParams: { ...fetchParams, signal } }) as Promise<ResourcePayload<R>>;
    },
    ...queryOptions,
  });
}
