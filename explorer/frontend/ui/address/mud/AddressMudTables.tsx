import { Box } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import useDebounce from 'lib/hooks/useDebounce';
import useIsInitialLoading from 'lib/hooks/useIsInitialLoading';
import getQueryParamString from 'lib/router/getQueryParamString';
import { ADDRESS_MUD_TABLE_ITEM } from 'stubs/address';
import { generateListStub } from 'stubs/utils';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

import AddressMudTablesListItem from './AddressMudTablesListItem';
import AddressMudTablesTable from './AddressMudTablesTable';

type Props = {
  isQueryEnabled?: boolean;
};

const AddressMudTables = ({ isQueryEnabled = true }: Props) => {
  const router = useRouter();

  const hash = getQueryParamString(router.query.hash);
  const q = getQueryParamString(router.query.q);
  const [ searchTerm, setSearchTerm ] = React.useState<string>(q || '');
  const debouncedSearchTerm = useDebounce(searchTerm, 300);

  const { data, isPlaceholderData, isError, pagination } = useQueryWithPages({
    resourceName: 'general:mud_tables',
    pathParams: { hash },
    filters: { q: debouncedSearchTerm },
    options: {
      enabled: isQueryEnabled,
      placeholderData: generateListStub<'general:mud_tables'>(ADDRESS_MUD_TABLE_ITEM, 3, { next_page_params: {
        items_count: 50,
        table_id: '1',
      } }),
    },
  });

  const isInitialLoading = useIsInitialLoading(isPlaceholderData);

  const searchInput = (
    <FilterInput
      w={{ base: '100%', lg: '360px' }}
      minW={{ base: 'auto', lg: '250px' }}
      size="sm"
      onChange={ setSearchTerm }
      placeholder="Search by name, namespace or table ID..."
      initialValue={ searchTerm }
      loading={ isInitialLoading }
    />
  );

  const actionBar = (
    <ActionBar mt={ -6 } showShadow justifyContent="space-between">
      { searchInput }
      <Pagination ml={{ base: 0, lg: 8 }} { ...pagination }/>
    </ActionBar>
  );

  const content = data?.items ? (
    <>
      <Box hideBelow="lg">
        <AddressMudTablesTable
          items={ data.items }
          isLoading={ isPlaceholderData }
          top={ ACTION_BAR_HEIGHT_DESKTOP }
          hash={ hash }
        />
      </Box>
      <Box hideFrom="lg">
        { data.items.map((item, index) => (
          <AddressMudTablesListItem
            key={ item.table.table_id + (isPlaceholderData ? String(index) : '') }
            item={ item }
            isLoading={ isPlaceholderData }
            hash={ hash }
          />
        )) }
      </Box>
    </>
  ) : null;

  return (
    <DataListDisplay
      isError={ isError }
      itemsNum={ data?.items?.length }
      emptyText="There are no tables for this address."
      hasActiveFilters={ Boolean(debouncedSearchTerm) }
      emptyStateProps={{
        term: 'table',
      }}
      actionBar={ actionBar }
    >
      { content }
    </DataListDisplay>
  );
};

export default AddressMudTables;
