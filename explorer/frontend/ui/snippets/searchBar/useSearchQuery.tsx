import { useRouter } from 'next/router';
import React from 'react';

import config from 'configs/app';
import { fromBech32Address, isBech32Address } from 'lib/address/bech32';
import useApiQuery from 'lib/api/useApiQuery';
import useDebounce from 'lib/hooks/useDebounce';
import useUpdateValueEffect from 'lib/hooks/useUpdateValueEffect';
import getQueryParamString from 'lib/router/getQueryParamString';
import { getExternalSearchItem } from 'lib/search/externalSearch';
import { SEARCH_RESULT_ITEM, SEARCH_RESULT_NEXT_PAGE_PARAMS } from 'stubs/search';
import { generateListStub } from 'stubs/utils';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

export default function useSearchQuery(withRedirectCheck?: boolean) {
  const router = useRouter();
  const q = React.useRef(getQueryParamString(router.query.q));
  const initialValue = q.current;

  const [ searchTerm, setSearchTerm ] = React.useState(initialValue);

  const debouncedSearchTerm = useDebounce(searchTerm, 300);
  const pathname = router.pathname;

  const query = useQueryWithPages({
    resourceName: 'general:search',
    filters: { q: isBech32Address(debouncedSearchTerm) ? fromBech32Address(debouncedSearchTerm) : debouncedSearchTerm },
    options: {
      enabled: debouncedSearchTerm.trim().length > 0,
      placeholderData: generateListStub<'general:search'>(SEARCH_RESULT_ITEM, 50, { next_page_params: SEARCH_RESULT_NEXT_PAGE_PARAMS }),
    },
  });

  const redirectCheckQuery = useApiQuery('general:search_check_redirect', {
    // on search result page we check redirect only once on mount
    queryParams: { q: q.current },
    queryOptions: { enabled: Boolean(q.current) && withRedirectCheck },
  });

  const zetaChainCCTXQuery = useApiQuery('zetachain:transactions', {
    queryParams: {
      hash: debouncedSearchTerm,
      limit: 50,
      offset: 0,
      direction: 'DESC',
    },
    queryOptions: { enabled: config.features.zetachain.isEnabled && debouncedSearchTerm.trim().length > 0 },
  });

  useUpdateValueEffect(() => {
    query.onFilterChange({ q: debouncedSearchTerm });
  }, debouncedSearchTerm);

  return React.useMemo(() => ({
    searchTerm,
    debouncedSearchTerm,
    handleSearchTermChange: setSearchTerm,
    query,
    redirectCheckQuery,
    pathname,
    zetaChainCCTXQuery,
    externalSearchItem: getExternalSearchItem(debouncedSearchTerm),
  }), [ debouncedSearchTerm, pathname, query, redirectCheckQuery, searchTerm, zetaChainCCTXQuery ]);
}
