import { useRouter } from 'next/router';
import React from 'react';

import { AddressFromToFilterValues, type AddressFromToFilter } from 'types/api/address';

import getFilterValueFromQuery from 'lib/getFilterValueFromQuery';
import getQueryParamString from 'lib/router/getQueryParamString';
import { INTERNAL_TX } from 'stubs/internalTx';
import { generateListStub } from 'stubs/utils';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

const getFilterValue = (getFilterValueFromQuery<AddressFromToFilter>).bind(null, AddressFromToFilterValues);

interface Props {
  enabled: boolean;
  isMultichain?: boolean;
  chainIds?: Array<string>;
}

export default function useAddressInternalTxsQuery({ enabled, isMultichain, chainIds }: Props) {
  const router = useRouter();
  const hash = getQueryParamString(router.query.hash);
  const [ filterValue, setFilterValue ] = React.useState<AddressFromToFilter>(getFilterValue(router.query.filter));

  const query = useQueryWithPages({
    resourceName: 'general:address_internal_txs',
    pathParams: { hash },
    filters: { filter: filterValue },
    options: {
      enabled,
      placeholderData: generateListStub<'general:address_internal_txs'>(
        INTERNAL_TX,
        50,
        {
          next_page_params: {
            block_number: 8987561,
            index: 2,
            items_count: 50,
            transaction_index: 67,
          },
        },
      ),
    },
    isMultichain,
    chainIds,
  });

  const onFilterChange = React.useCallback((val: string | Array<string>) => {
    const newVal = getFilterValue(val);
    setFilterValue(newVal);
    query.onFilterChange({ filter: newVal });
  }, [ query ]);

  return React.useMemo(() => ({
    hash,
    query,
    filterValue,
    onFilterChange,
  }), [ query, filterValue, onFilterChange, hash ]);
}
