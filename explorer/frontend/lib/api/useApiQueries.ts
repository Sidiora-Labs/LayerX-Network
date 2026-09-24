import type { UseQueryOptions } from '@tanstack/react-query';
import { useQueries } from '@tanstack/react-query';

import type { ResourceError, ResourceName, ResourcePayload } from './resources';
import useApiFetch from './useApiFetch';
import type { Params as ApiQueryParams } from './useApiQuery';
import { getResourceKey } from './useApiQuery';

export interface ReturnType<R extends ResourceName, E = unknown> {
  isLoading: boolean;
  isPlaceholderData: boolean;
  isError: boolean;
  isSuccess: boolean;
  data: Array<ResourcePayload<R>> | undefined;
  error: ResourceError<E> | null | undefined;
}

export default function useApiQueries<R extends ResourceName, E = unknown>(
  resource: R,
  params: Array<ApiQueryParams<R, E>>,
  queryOptions: Partial<Omit<UseQueryOptions<ResourcePayload<R>, ResourceError<E>, Array<ResourcePayload<R>>>, 'queries'>>,
) {
  const apiFetch = useApiFetch();

  return useQueries<Array<UseQueryOptions<ResourcePayload<R>, ResourceError<E>>>, ReturnType<R, E>>({
    queries: params.map(({ pathParams, queryParams, queryOptions, fetchParams, logError, chain }) => {
      return {
        queryKey: getResourceKey(resource, { pathParams, queryParams, chainId: chain?.id }),
        queryFn: async({ signal }) => {
          return apiFetch(resource, { pathParams, queryParams, logError, chain, fetchParams: { ...fetchParams, signal } }) as Promise<ResourcePayload<R>>;
        },
        ...queryOptions,
      };
    }),
    combine: (results) => {
      const isError = results.some((result) => result.isError);
      const isSuccess = results.every((result) => result.isSuccess);

      return {
        isLoading: results.some((result) => result.isLoading),
        isPlaceholderData: results.some((result) => result.isPlaceholderData),
        isError,
        isSuccess,
        data: isSuccess ? results.map((result) => result.data).flat() as Array<ResourcePayload<R>> : undefined,
        error: isError ? results.find((result) => result.error)?.error : undefined,
      } satisfies ReturnType<R, E>;
    },
    ...queryOptions,
  });
}
