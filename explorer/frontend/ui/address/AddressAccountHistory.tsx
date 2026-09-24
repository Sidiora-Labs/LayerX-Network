import { Box } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type { NovesHistoryFilterValue } from 'types/api/noves';
import { NovesHistoryFilterValues } from 'types/api/noves';

import getFilterValueFromQuery from 'lib/getFilterValueFromQuery';
import useIsMounted from 'lib/hooks/useIsMounted';
import getQueryParamString from 'lib/router/getQueryParamString';
import { NOVES_TRANSLATE } from 'stubs/noves/NovesTranslate';
import { generateListStub } from 'stubs/utils';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import AddressAccountHistoryTableItem from 'ui/address/accountHistory/AddressAccountHistoryTableItem';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import { getFromToValue } from 'ui/shared/Noves/utils';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

import AddressAccountHistoryListItem from './accountHistory/AddressAccountHistoryListItem';
import AccountHistoryFilter from './AddressAccountHistoryFilter';

const getFilterValue = (getFilterValueFromQuery<NovesHistoryFilterValue>).bind(null, NovesHistoryFilterValues);

type Props = {
  shouldRender?: boolean;
  isQueryEnabled?: boolean;
};

const AddressAccountHistory = ({ shouldRender = true, isQueryEnabled = true }: Props) => {
  const router = useRouter();
  const isMounted = useIsMounted();

  const currentAddress = getQueryParamString(router.query.hash).toLowerCase();

  const [ filterValue, setFilterValue ] = React.useState<NovesHistoryFilterValue>(getFilterValue(router.query.filter));

  const { data, isError, pagination, isPlaceholderData } = useQueryWithPages({
    resourceName: 'general:noves_address_history',
    pathParams: { address: currentAddress },
    options: {
      enabled: isQueryEnabled,
      placeholderData: generateListStub<'general:noves_address_history'>(NOVES_TRANSLATE, 10, { hasNextPage: false, pageNumber: 1, pageSize: 10 }),
    },
  });

  const handleFilterChange = React.useCallback((val: string | Array<string>) => {

    const newVal = getFilterValue(val);
    setFilterValue(newVal);
  }, [ ]);

  if (!isMounted || !shouldRender) {
    return null;
  }

  const actionBar = (
    <ActionBar mt={ -6 } pb={{ base: 6, md: 5 }}>
      <AccountHistoryFilter
        defaultFilter={ filterValue }
        onFilterChange={ handleFilterChange }
        hasActiveFilter={ Boolean(filterValue) }
        isLoading={ pagination.isLoading }
      />

      <Pagination ml={{ base: 'auto', lg: 8 }} { ...pagination }/>
    </ActionBar>
  );

  const filteredData = isPlaceholderData ? data?.items : data?.items.filter(i => filterValue ? getFromToValue(i, currentAddress) === filterValue : i);

  const content = (
    <Box position="relative">
      <Box hideFrom="lg">
        { filteredData?.map((item, i) => (
          <AddressAccountHistoryListItem
            key={ `${ i }-${ item.rawTransactionData.transactionHash }` }
            tx={ item }
            currentAddress={ currentAddress }
            isPlaceholderData={ isPlaceholderData }
          />
        )) }
      </Box>

      <Box hideBelow="lg">
        <TableRoot>
          <TableHeaderSticky top={ 75 }>
            <TableRow>
              <TableColumnHeader width="120px">
                Age
              </TableColumnHeader>
              <TableColumnHeader>
                Action
              </TableColumnHeader>
              <TableColumnHeader width="320px">
                From/To
              </TableColumnHeader>
            </TableRow>
          </TableHeaderSticky>
          <TableBody maxWidth="full">
            { filteredData?.map((item, i) => (
              <AddressAccountHistoryTableItem
                key={ `${ i }-${ item.rawTransactionData.transactionHash }` }
                tx={ item }
                currentAddress={ currentAddress }
                isPlaceholderData={ isPlaceholderData }
              />
            )) }
          </TableBody>
        </TableRoot>
      </Box>
    </Box>
  );

  return (
    <DataListDisplay
      isError={ isError }
      itemsNum={ filteredData?.length }
      emptyText="There are no transactions."
      actionBar={ actionBar }
      hasActiveFilters={ Boolean(filterValue) }
      emptyStateProps={{
        description: 'No match found for current filter',
      }}
    >
      { content }
    </DataListDisplay>
  );
};

export default AddressAccountHistory;
