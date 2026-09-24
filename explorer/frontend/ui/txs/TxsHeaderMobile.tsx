import { HStack, chakra, createListCollection } from '@chakra-ui/react';
import React from 'react';

import type { TransactionsSortingValue } from 'types/api/transaction';
import type { PaginationParams } from 'ui/shared/pagination/types';

// import { FilterInput } from 'toolkit/components/filters/FilterInput';

import ActionBar from 'ui/shared/ActionBar';
import Pagination from 'ui/shared/pagination/Pagination';
import Sort from 'ui/shared/sort/Sort';

import { SORT_OPTIONS } from './useTxsSort';

type Props = {
  sorting: TransactionsSortingValue;
  setSorting?: (val: TransactionsSortingValue) => void;
  paginationProps: PaginationParams;
  className?: string;
  showPagination?: boolean;
  filterComponent?: React.ReactNode;
  linkSlot?: React.ReactNode;
  tableViewButton?: React.ReactNode;
};

const collection = createListCollection({
  items: SORT_OPTIONS,
});

const TxsHeaderMobile = ({ filterComponent, sorting, setSorting, paginationProps, className, showPagination = true, linkSlot, tableViewButton }: Props) => {
  const handleSortValueChange = React.useCallback(({ value }: { value: Array<string> }) => {
    setSorting?.(value[0] as TransactionsSortingValue);
  }, [ setSorting ]);

  if (!filterComponent && !setSorting && !linkSlot && !showPagination && !tableViewButton) {
    return null;
  }

  return (
    <ActionBar className={ className }>
      <HStack>
        { tableViewButton }
        { filterComponent }
        { setSorting && (
          <Sort
            name="transactions_sorting"
            defaultValue={ [ sorting ] }
            collection={ collection }
            onValueChange={ handleSortValueChange }
            isLoading={ paginationProps.isLoading }
          />
        ) }
        { /* api is not implemented */ }
        { /* <FilterInput
          // eslint-disable-next-line react/jsx-no-bind
          onChange={ () => {} }
          maxW="360px"
          size="xs"
          placeholder="Search by addresses, hash, method..."
        /> */ }
        { linkSlot }
      </HStack>
      { showPagination && <Pagination { ...paginationProps }/> }
    </ActionBar>
  );
};

export default chakra(TxsHeaderMobile);
