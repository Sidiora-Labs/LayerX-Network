import { Box, Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import config from 'configs/app';
import useDebounce from 'lib/hooks/useDebounce';
import getQueryParamString from 'lib/router/getQueryParamString';
import { POOL } from 'stubs/pools';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import PoolsListItem from 'ui/pools/PoolsListItem';
import PoolsTable from 'ui/pools/PoolsTable';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

const Pools = () => {
  const router = useRouter();
  const q = getQueryParamString(router.query.query);

  const [ searchTerm, setSearchTerm ] = React.useState<string>(q ?? '');
  const debouncedSearchTerm = useDebounce(searchTerm, 300);

  const poolsQuery = useQueryWithPages({
    resourceName: 'contractInfo:pools',
    pathParams: { chainId: config.chain.id },
    filters: { query: debouncedSearchTerm },
    options: {
      placeholderData: { items: Array(50).fill(POOL), next_page_params: { page_token: 'a', page_size: 50 } },
    },
  });

  const handleSearchTermChange = React.useCallback((value: string) => {
    poolsQuery.onFilterChange({ query: value });
    setSearchTerm(value);
  }, [ poolsQuery ]);

  const content = (
    <>
      <Box hideFrom="lg">
        { poolsQuery.data?.items.map((item, index) => (
          <PoolsListItem
            key={ item.pool_id + (poolsQuery.isPlaceholderData ? index : '') }
            isLoading={ poolsQuery.isPlaceholderData }
            item={ item }
          />
        )) }
      </Box>
      <Box hideBelow="lg">
        <PoolsTable
          items={ poolsQuery.data?.items ?? [] }
          top={ poolsQuery.pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }
          isLoading={ poolsQuery.isPlaceholderData }
          page={ poolsQuery.pagination.page }
        />
      </Box>
    </>
  );

  const filter = (
    <FilterInput
      w={{ base: '100%', lg: '360px' }}
      size="sm"
      onChange={ handleSearchTermChange }
      placeholder="Pair, token symbol or token address"
      initialValue={ searchTerm }
    />
  );

  const actionBar = (
    <>
      <Flex mb={ 6 } display={{ base: 'flex', lg: 'none' }}>
        { filter }
      </Flex>
      <ActionBar
        mt={ -6 }
        display={{ base: poolsQuery.pagination.isVisible ? 'flex' : 'none', lg: 'flex' }}
      >
        <Box hideBelow="lg">
          { filter }
        </Box>
        <Pagination { ...poolsQuery.pagination } ml="auto"/>
      </ActionBar>
    </>
  );

  return (
    <>
      <PageTitle
        title="DEX tracker"
        withTextAd
      />
      <DataListDisplay
        isError={ poolsQuery.isError }
        itemsNum={ poolsQuery.data?.items.length }
        emptyText="There are no pools."
        actionBar={ actionBar }
        hasActiveFilters={ Boolean(debouncedSearchTerm) }
        emptyStateProps={{
          term: 'pool',
        }}
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default Pools;
