import { useRouter } from 'next/router';
import React from 'react';

import type { AddressFromToFilter } from 'types/api/address';
import { AddressFromToFilterValues } from 'types/api/address';
import type { TokenType } from 'types/api/token';

import multichainConfig from 'configs/multichain';
import getFilterValueFromQuery from 'lib/getFilterValueFromQuery';
import getFilterValuesFromQuery from 'lib/getFilterValuesFromQuery';
import getQueryParamString from 'lib/router/getQueryParamString';
import { getTokenTypes } from 'lib/token/tokenTypes';
import { getTokenTransfersStub } from 'stubs/token';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

export type Filters = {
  type: Array<TokenType>;
  filter: AddressFromToFilter | undefined;
};

const getAddressFilterValue = (getFilterValueFromQuery<AddressFromToFilter>).bind(null, AddressFromToFilterValues);

const getFilters = (query: Record<string, string | Array<string> | undefined>, chainIds?: Array<string>) => {
  const chainIdParam = getQueryParamString(query.chain_id);
  const filterParam = getQueryParamString(query.filter);
  const typeParam = getQueryParamString(query.type);

  const currentChainId = chainIdParam && chainIds?.includes(chainIdParam) ? chainIdParam : chainIds?.[0];
  const chainConfig = multichainConfig()?.chains.find(chain => chain.id === currentChainId);

  return {
    filter: getAddressFilterValue(filterParam),
    type: getFilterValuesFromQuery(
      Object.keys(getTokenTypes(false, chainConfig?.app_config)),
      typeParam,
    ) || [],
  };
};

interface Props {
  currentAddress: string;
  enabled?: boolean;
  isMultichain?: boolean;
  chainIds?: Array<string>;
}

export default function useAddressTokenTransfersQuery({ currentAddress, enabled, isMultichain, chainIds }: Props) {
  const router = useRouter();

  const [ filters, setFilters ] = React.useState<Filters>(getFilters(router.query, chainIds));

  React.useEffect(() => {
    if (enabled) {
      setFilters(getFilters(router.query, chainIds));
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ enabled ]);

  const query = useQueryWithPages({
    resourceName: 'general:address_token_transfers',
    pathParams: { hash: currentAddress },
    filters,
    options: {
      enabled,
      placeholderData: getTokenTransfersStub(undefined, {
        block_number: 7793535,
        index: 46,
        items_count: 50,
      }),
    },
    isMultichain,
    chainIds,
  });

  const onTypeFilterChange = React.useCallback((nextValue: Array<TokenType>) => {
    query.onFilterChange({ ...filters, type: nextValue });
    setFilters((prevState) => ({ ...prevState, type: nextValue }));
  }, [ filters, query ]);

  const onAddressFilterChange = React.useCallback((nextValue: string) => {
    const filterVal = getAddressFilterValue(nextValue);
    query.onFilterChange({ ...filters, filter: filterVal });
    setFilters((prevState) => ({ ...prevState, filter: filterVal }));
  }, [ filters, query ]);

  return React.useMemo(() => ({
    query,
    filters,
    setFilters,
    onTypeFilterChange,
    onAddressFilterChange,
  }), [ query, filters, setFilters, onTypeFilterChange, onAddressFilterChange ]);
}
