import { useRouter } from 'next/router';
import React from 'react';

import type { VerifiedContractsFilters } from 'types/api/contracts';
import type { VerifiedContractsSorting, VerifiedContractsSortingField, VerifiedContractsSortingValue } from 'types/api/verifiedContracts';

import useDebounce from 'lib/hooks/useDebounce';
import getQueryParamString from 'lib/router/getQueryParamString';
import { VERIFIED_CONTRACT_INFO } from 'stubs/contract';
import { generateListStub } from 'stubs/utils';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import getSortParamsFromValue from 'ui/shared/sort/getSortParamsFromValue';
import getSortValueFromQuery from 'ui/shared/sort/getSortValueFromQuery';

import { SORT_OPTIONS } from './utils';

interface Props {
  isMultichain?: boolean;
}

export default function useVerifiedContractsQuery({ isMultichain }: Props = {}) {
  const router = useRouter();
  const [ searchTerm, setSearchTerm ] = React.useState(getQueryParamString(router.query.q) || undefined);
  const [ type, setType ] = React.useState(getQueryParamString(router.query.filter) as VerifiedContractsFilters['filter'] || undefined);
  const [ sort, setSort ] =
      React.useState<VerifiedContractsSortingValue>(getSortValueFromQuery<VerifiedContractsSortingValue>(router.query, SORT_OPTIONS) ?? 'default');

  const debouncedSearchTerm = useDebounce(searchTerm || '', 300);

  const query = useQueryWithPages({
    resourceName: 'general:verified_contracts',
    filters: { q: debouncedSearchTerm, filter: type },
    sorting: getSortParamsFromValue<VerifiedContractsSortingValue, VerifiedContractsSortingField, VerifiedContractsSorting['order']>(sort),
    options: {
      placeholderData: generateListStub<'general:verified_contracts'>(
        VERIFIED_CONTRACT_INFO,
        50,
        {
          next_page_params: {
            items_count: '50',
            smart_contract_id: '50',
          },
        },
      ),
    },
    isMultichain,
  });
  const { onFilterChange, onSortingChange } = query;

  const onSearchTermChange = React.useCallback((value: string) => {
    onFilterChange({ q: value, filter: type });
    setSearchTerm(value);
  }, [ type, onFilterChange ]);

  const onTypeChange = React.useCallback((value: string | Array<string>) => {
    if (Array.isArray(value)) {
      return;
    }

    const filter = value === 'all' ? undefined : value as VerifiedContractsFilters['filter'];

    onFilterChange({ q: debouncedSearchTerm, filter });
    setType(filter);
  }, [ debouncedSearchTerm, onFilterChange ]);

  const onSortChange = React.useCallback(({ value }: { value: Array<string> }) => {
    setSort(value[0] as VerifiedContractsSortingValue);
    onSortingChange(value[0] === 'default' ? undefined : getSortParamsFromValue(value[0] as VerifiedContractsSortingValue));
  }, [ onSortingChange ]);

  return React.useMemo(() => ({
    query,
    type,
    searchTerm,
    debouncedSearchTerm,
    sort,
    onSearchTermChange,
    onTypeChange,
    onSortChange,
  }), [ query, type, searchTerm, debouncedSearchTerm, sort, onSearchTermChange, onTypeChange, onSortChange ]);
}
