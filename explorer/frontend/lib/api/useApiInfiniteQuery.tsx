import type { InfiniteData, QueryKey, UseInfiniteQueryResult, UseInfiniteQueryOptions } from '@tanstack/react-query';
import { useInfiniteQuery } from '@tanstack/react-query';

import type { PaginatedResourceName, ResourceError, ResourcePayload } from './resources';
import useApiFetch from './useApiFetch';
import type { Params as ApiFetchParams } from './useApiFetch';
import { getResourceKey } from './useApiQuery';

type TQueryData<R extends PaginatedResourceName> = ResourcePayload<R>;
type TError = ResourceError<unknown>;
type TPageParam<R extends PaginatedResourceName> = ApiFetchParams<R>['queryParams'] | null;

export interface Params<R extends PaginatedResourceName> {
  resourceName: R;
  // eslint-disable-next-line max-len
  queryOptions?: Omit<UseInfiniteQueryOptions<TQueryData<R>, TError, InfiniteData<TQueryData<R>>, TQueryData<R>, QueryKey, TPageParam<R>>, 'queryKey' | 'queryFn' | 'getNextPageParam' | 'initialPageParam'>;
  pathParams?: ApiFetchParams<R>['pathParams'];
  queryParams?: Record<string, string | Array<string> | number | boolean | undefined | null>;
}

export type ReturnType<Resource extends PaginatedResourceName> = UseInfiniteQueryResult<InfiniteData<ResourcePayload<Resource>>, ResourceError<unknown>>;

export default function useApiInfiniteQuery<R extends PaginatedResourceName>({
  resourceName,
  queryOptions,
  pathParams,
  queryParams,
}: Params<R>): ReturnType<R> {
  const apiFetch = useApiFetch();

  return useInfiniteQuery<TQueryData<R>, TError, InfiniteData<TQueryData<R>>, QueryKey, TPageParam<R>>({
    queryKey: getResourceKey(resourceName, { pathParams, queryParams }),
    queryFn: (context) => {
      const nextQueryParams = 'pageParam' in context ? (context.pageParam || undefined) : undefined;
      return apiFetch(resourceName, { pathParams, queryParams: { ...queryParams, ...nextQueryParams } }) as Promise<TQueryData<R>>;
    },
    initialPageParam: null,
    getNextPageParam: (lastPage) => {
      return lastPage.next_page_params as TPageParam<R>;
    },
    ...queryOptions,
  });
}
