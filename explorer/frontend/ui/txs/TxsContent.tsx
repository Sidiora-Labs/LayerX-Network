import { Box } from '@chakra-ui/react';
import React from 'react';

import type { TxsSocketType } from './socket/types';
import type { AddressFromToFilter } from 'types/api/address';
import type { Transaction, TransactionsSortingField, TransactionsSortingValue } from 'types/api/transaction';
import type { PaginationParams } from 'ui/shared/pagination/types';

import useIsMobile from 'lib/hooks/useIsMobile';
import useTableViewValue from 'lib/hooks/useTableViewValue';
import AddressCsvExportLink from 'ui/address/AddressCsvExportLink';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import getNextSortValue from 'ui/shared/sort/getNextSortValue';
import TableViewToggleButton from 'ui/shared/TableViewToggleButton';

import useDescribeTxs from './noves/useDescribeTxs';
import TxsHeaderMobile from './TxsHeaderMobile';
import TxsList from './TxsList';
import TxsTable from './TxsTable';

const SORT_SEQUENCE: Record<TransactionsSortingField, Array<TransactionsSortingValue>> = {
  value: [ 'value-desc', 'value-asc', 'default' ],
  fee: [ 'fee-desc', 'fee-asc', 'default' ],
  block_number: [ 'block_number-asc', 'default' ],
};

type Props = {
  pagination: PaginationParams;
  showBlockInfo?: boolean;
  socketType?: TxsSocketType;
  currentAddress?: string;
  filter?: React.ReactNode;
  filterValue?: AddressFromToFilter;
  enableTimeIncrement?: boolean;
  top?: number;
  items?: Array<Transaction>;
  isPlaceholderData: boolean;
  isError: boolean;
  setSorting?: (value: TransactionsSortingValue) => void;
  sort: TransactionsSortingValue;
  stickyHeader?: boolean;
  showTableViewButton?: boolean;
};

const TxsContent = ({
  pagination,
  filter,
  filterValue,
  showBlockInfo = true,
  socketType,
  currentAddress,
  enableTimeIncrement,
  top,
  items,
  isPlaceholderData,
  isError,
  setSorting,
  sort,
  stickyHeader = true,
  showTableViewButton,
}: Props) => {
  const isMobile = useIsMobile();

  const tableViewFlag = useTableViewValue();

  const isTableView = isMobile ? showTableViewButton && !tableViewFlag.isLoading && tableViewFlag.value : true;
  const isLoading = isPlaceholderData || tableViewFlag.isLoading;

  const onSortToggle = React.useCallback((field: TransactionsSortingField) => {
    const value = getNextSortValue<TransactionsSortingField, TransactionsSortingValue>(SORT_SEQUENCE, field)(sort);
    setSorting?.(value);
  }, [ sort, setSorting ]);

  const translationQuery = useDescribeTxs(items, currentAddress, isPlaceholderData);

  const content = items && items.length > 0 ? (
    <>
      <Box display={ isTableView ? 'none' : 'block' }>
        <TxsList
          showBlockInfo={ showBlockInfo }
          socketType={ socketType }
          isLoading={ isLoading }
          enableTimeIncrement={ enableTimeIncrement }
          currentAddress={ currentAddress }
          items={ items }
          translationQuery={ translationQuery }
        />
      </Box>
      <Box
        display={ isTableView ? 'block' : 'none' }
        overflowX={ isMobile ? 'scroll' : undefined }
        mx={ isMobile ? -3 : 0 }
        px={ isMobile ? 3 : 0 }
      >
        <TxsTable
          txs={ items }
          sort={ sort }
          onSortToggle={ setSorting ? onSortToggle : undefined }
          showBlockInfo={ showBlockInfo }
          socketType={ socketType }
          top={ top || (pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0) }
          currentAddress={ currentAddress }
          enableTimeIncrement={ enableTimeIncrement }
          isLoading={ isLoading }
          stickyHeader={ !isMobile && stickyHeader }
          translationQuery={ translationQuery }
        />
      </Box>
    </>
  ) : null;

  const tableViewButton = isMobile && showTableViewButton ? (
    <TableViewToggleButton
      value={ tableViewFlag.value }
      onClick={ tableViewFlag.onToggle }
      loading={ isLoading }
    />
  ) : null;

  const actionBar = isMobile ? (
    <TxsHeaderMobile
      mt={ -6 }
      sorting={ sort }
      setSorting={ setSorting }
      paginationProps={ pagination }
      showPagination={ pagination.isVisible }
      filterComponent={ filter }
      linkSlot={ currentAddress ? (
        <AddressCsvExportLink
          address={ currentAddress }
          params={{ type: 'transactions', filterType: 'address', filterValue }}
          isLoading={ pagination.isLoading }
        />
      ) : null }
      tableViewButton={ tableViewButton }
    />
  ) : null;

  return (
    <DataListDisplay
      isError={ isError }
      itemsNum={ items?.length }
      emptyText="There are no transactions."
      actionBar={ actionBar }
      hasActiveFilters={ Boolean(filterValue) }
      emptyStateProps={{
        term: 'transaction',
      }}
    >
      { content }
    </DataListDisplay>
  );
};

export default TxsContent;
