import { useRouter } from 'next/router';
import React from 'react';

import useApiInfiniteQuery from 'lib/api/useApiInfiniteQuery';
import useApiQuery from 'lib/api/useApiQuery';
import useDebounce from 'lib/hooks/useDebounce';
import getQueryParamString from 'lib/router/getQueryParamString';

interface Props {
  chainId: string | undefined;
}

export default function useSearchQuery({ chainId }: Props) {
  const router = useRouter();
  const q = React.useRef(getQueryParamString(router.query.q));
  const initialValue = q.current;
  const checkRedirect = getQueryParamString(router.query.redirect) === 'true';

  const [ searchTerm, setSearchTerm ] = React.useState(initialValue);

  const debouncedSearchTerm = useDebounce(searchTerm, 300);

  const checkRedirectQuery = useApiQuery('multichainAggregator:search_check_redirect', {
    // on search result page we check redirect only once on mount
    queryParams: { q: initialValue },
    queryOptions: { enabled: Boolean(initialValue) && checkRedirect },
  });

  const addressesQuery = useApiInfiniteQuery({
    resourceName: 'multichainAggregator:search_addresses',
    queryParams: { q: debouncedSearchTerm, chain_id: chainId },
  });

  const tokensQuery = useApiInfiniteQuery({
    resourceName: 'multichainAggregator:search_tokens',
    queryParams: { q: debouncedSearchTerm, chain_id: chainId },
  });

  const blockNumbersQuery = useApiInfiniteQuery({
    resourceName: 'multichainAggregator:search_block_numbers',
    queryParams: { q: debouncedSearchTerm, chain_id: chainId },
  });

  const blocksQuery = useApiInfiniteQuery({
    resourceName: 'multichainAggregator:search_blocks',
    queryParams: { q: debouncedSearchTerm, chain_id: chainId },
  });

  const nftsQuery = useApiInfiniteQuery({
    resourceName: 'multichainAggregator:search_nfts',
    queryParams: { q: debouncedSearchTerm, chain_id: chainId },
  });

  const transactionsQuery = useApiInfiniteQuery({
    resourceName: 'multichainAggregator:search_transactions',
    queryParams: { q: debouncedSearchTerm, chain_id: chainId },
  });

  const domainsQuery = useApiInfiniteQuery({
    resourceName: 'multichainAggregator:search_domains',
    queryParams: { q: debouncedSearchTerm, chain_id: chainId },
  });

  const handleSearchTermChange = React.useCallback((value: string) => {
    setSearchTerm(value);
    router.push({
      pathname: router.pathname,
      query: { ...router.query, q: value },
    });
  }, [ router ]);

  const handleSubmit = React.useCallback((event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
  }, [ ]);

  const queries = React.useMemo(() => ({
    addresses: addressesQuery,
    tokens: tokensQuery,
    blockNumbers: blockNumbersQuery,
    blocks: blocksQuery,
    nfts: nftsQuery,
    transactions: transactionsQuery,
    domains: domainsQuery,
  }), [ addressesQuery, tokensQuery, blockNumbersQuery, blocksQuery, nftsQuery, transactionsQuery, domainsQuery ]);

  return React.useMemo(() => ({
    searchTerm,
    debouncedSearchTerm,
    handleSearchTermChange,
    handleSubmit,
    queries,
    checkRedirectQuery,
  }), [
    queries,
    debouncedSearchTerm,
    handleSearchTermChange,
    handleSubmit,
    searchTerm,
    checkRedirectQuery,
  ]);
}
