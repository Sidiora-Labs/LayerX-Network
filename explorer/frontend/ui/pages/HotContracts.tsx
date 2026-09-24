import { Box, createListCollection, Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type { HotContractsInterval, HotContractsSorting, HotContractsSortingField, HotContractsSortingValue } from 'types/api/contracts';

import useApiQuery from 'lib/api/useApiQuery';
import { HOT_CONTRACTS } from 'stubs/contract';
import { HOMEPAGE_STATS } from 'stubs/stats';
import { Skeleton } from 'toolkit/chakra/skeleton';
import HotContractsIntervalSelect from 'ui/hotContracts/HotContractsIntervalSelect';
import HotContractsListItem from 'ui/hotContracts/HotContractsListItem';
import HotContractsTable from 'ui/hotContracts/HotContractsTable';
import { getIntervalValueFromQuery, SORT_OPTIONS } from 'ui/hotContracts/utils';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import getSortParamsFromValue from 'ui/shared/sort/getSortParamsFromValue';
import getSortValueFromQuery from 'ui/shared/sort/getSortValueFromQuery';
import Sort from 'ui/shared/sort/Sort';

const sortCollection = createListCollection({
  items: SORT_OPTIONS,
});

const HotContracts = () => {
  const router = useRouter();
  const [ interval, setInterval ] = React.useState<HotContractsInterval>(getIntervalValueFromQuery(router.query.scale));
  const [ sort, setSort ] =
      React.useState<HotContractsSortingValue>(getSortValueFromQuery<HotContractsSortingValue>(router.query, SORT_OPTIONS) ?? 'default');

  const { data, isError, isPlaceholderData, pagination, onSortingChange, onFilterChange } = useQueryWithPages({
    resourceName: 'general:stats_hot_contracts',
    filters: { scale: interval },
    sorting: getSortParamsFromValue<HotContractsSortingValue, HotContractsSortingField, HotContractsSorting['order']>(sort),
    options: {
      placeholderData: {
        items: Array(50).fill(HOT_CONTRACTS),
        next_page_params: { items_count: '50', transactions_count: '50', total_gas_used: '50', contract_address_hash: '50' },
      },
    },
  });

  const statsQuery = useApiQuery('general:stats', {
    queryOptions: {
      placeholderData: HOMEPAGE_STATS,
      refetchOnMount: false,
    },
  });

  const handleSortChange = React.useCallback(({ value }: { value: Array<string> }) => {
    setSort(value[0] as HotContractsSortingValue);
    onSortingChange(value[0] === 'default' ? undefined : getSortParamsFromValue(value[0] as HotContractsSortingValue));
  }, [ onSortingChange ]);

  const handleIntervalChange = React.useCallback((newInterval: HotContractsInterval) => {
    setInterval(newInterval);
    onFilterChange({ scale: newInterval });
  }, [ onFilterChange ]);

  const content = (
    <>
      <Box hideFrom="lg">
        { data?.items.map((item, index) => (
          <HotContractsListItem
            key={ item.contract_address.hash + (isPlaceholderData ? index : '') }
            isLoading={ isPlaceholderData }
            data={ item }
            exchangeRate={ statsQuery.data?.coin_price ?? null }
          />
        )) }
      </Box>
      <Box hideBelow="lg">
        <HotContractsTable
          items={ data?.items }
          isLoading={ isPlaceholderData }
          sort={ sort }
          setSorting={ handleSortChange }
          exchangeRate={ statsQuery.data?.coin_price ?? null }
        />
      </Box>
    </>
  );

  const actionBar = (
    <ActionBar mt={ -6 }>
      <Flex alignItems="center">
        <HotContractsIntervalSelect
          interval={ interval }
          onIntervalChange={ handleIntervalChange }
          isLoading={ isPlaceholderData }
        />
        { [ '1d', '7d', '30d' ].includes(interval) && (
          <Skeleton loading={ isPlaceholderData } color="text.secondary" hideBelow="lg" textStyle="sm" ml={ 6 }>
            <span>The data is updated once a day.</span>
          </Skeleton>
        ) }
        <Sort
          name="hot_contracts_sorting"
          defaultValue={ [ sort ] }
          collection={ sortCollection }
          onValueChange={ handleSortChange }
          isLoading={ isPlaceholderData }
          hideFrom="lg"
          ml={ 2 }
        />
      </Flex>
      <Pagination { ...pagination } ml="auto"/>
    </ActionBar>
  );

  return (
    <>
      <PageTitle
        title="Hot contracts"
        withTextAd
      />
      <DataListDisplay
        isError={ isError }
        itemsNum={ data?.items.length }
        emptyText="There are no hot contracts."
        actionBar={ actionBar }
        hasActiveFilters={ Boolean(interval) }
        emptyStateProps={{
          term: 'hot contract',
        }}
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default HotContracts;
