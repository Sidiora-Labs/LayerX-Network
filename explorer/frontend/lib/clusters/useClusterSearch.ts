import { useRouter } from 'next/router';

import useDebounce from 'lib/hooks/useDebounce';
import getQueryParamString from 'lib/router/getQueryParamString';

export function useClusterSearch() {
  const router = useRouter();
  const searchTerm = getQueryParamString(router.query.q);
  const debouncedSearchTerm = useDebounce(searchTerm || '', 300);

  return {
    searchTerm,
    debouncedSearchTerm,
  };
}
