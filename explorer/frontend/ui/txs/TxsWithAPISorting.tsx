import React from 'react';

import type { TxsSocketType } from './socket/types';
import type { AddressFromToFilter } from 'types/api/address';
import type { TransactionsSortingValue } from 'types/api/transaction';

import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';
import getSortParamsFromValue from 'ui/shared/sort/getSortParamsFromValue';

import TxsContent from './TxsContent';

type Props = {

  query: QueryWithPagesResult<'general:address_txs'>;
  showBlockInfo?: boolean;
  socketType?: TxsSocketType;
  currentAddress?: string;
  filter?: React.ReactNode;
  filterValue?: AddressFromToFilter;
  enableTimeIncrement?: boolean;
  top?: number;
  sorting: TransactionsSortingValue;
  setSort: (value: TransactionsSortingValue) => void;
  showTableViewButton?: boolean;
};

const TxsWithAPISorting = ({
  filter,
  filterValue,
  query,
  showBlockInfo = true,
  socketType,
  currentAddress,
  enableTimeIncrement,
  top,
  sorting,
  setSort,
  showTableViewButton,
}: Props) => {

  const handleSortChange = React.useCallback((value: TransactionsSortingValue) => {
    setSort(value);
    query.onSortingChange(getSortParamsFromValue(value));
  }, [ setSort, query ]);

  return (
    <TxsContent
      filter={ filter }
      filterValue={ filterValue }
      showBlockInfo={ showBlockInfo }
      socketType={ socketType }
      currentAddress={ currentAddress }
      enableTimeIncrement={ enableTimeIncrement }
      top={ top }
      items={ query.data?.items }
      isPlaceholderData={ query.isPlaceholderData }
      isError={ query.isError }
      setSorting={ handleSortChange }
      sort={ sorting }
      pagination={ query.pagination }
      showTableViewButton={ showTableViewButton }
    />
  );
};

export default TxsWithAPISorting;
