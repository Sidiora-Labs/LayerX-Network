import { useRouter } from 'next/router';
import { useCallback, useMemo } from 'react';

import type { PaginationParams } from 'ui/shared/pagination/types';

import getQueryParamString from 'lib/router/getQueryParamString';
import { useQueryParams } from 'lib/router/useQueryParams';

export function useClusterPagination(hasNextPage: boolean, isLoading: boolean) {
  const router = useRouter();
  const { updateQuery } = useQueryParams();
  const page = parseInt(getQueryParamString(router.query.page) || '1', 10);

  const onNextPageClick = useCallback(() => {
    updateQuery({ page: (page + 1).toString() });
  }, [ updateQuery, page ]);

  const onPrevPageClick = useCallback(() => {
    updateQuery({ page: page === 2 ? undefined : (page - 1).toString() });
  }, [ updateQuery, page ]);

  const resetPage = useCallback(() => {
    updateQuery({ page: undefined });
  }, [ updateQuery ]);

  const pagination: PaginationParams = useMemo(() => ({
    page,
    onNextPageClick,
    onPrevPageClick,
    resetPage,
    hasPages: page > 1,
    hasNextPage,
    canGoBackwards: page > 1,
    isLoading,
    isVisible: page > 1 || hasNextPage,
  }), [ page, onNextPageClick, onPrevPageClick, resetPage, hasNextPage, isLoading ]);

  return {
    page,
    pagination,
  };
}
