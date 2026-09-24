import {
  Box,
  Text,
  chakra,
  Flex,
  HStack,
} from '@chakra-ui/react';
import { omit } from 'es-toolkit';
import { useRouter } from 'next/router';
import React from 'react';

import type { AdvancedFilterParams } from 'types/api/advancedFilter';
import { ADVANCED_FILTER_AGES, ADVANCED_FILTER_ADDRESS_RELATION } from 'types/api/advancedFilter';

import useApiQuery from 'lib/api/useApiQuery';
import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import { useMultichainContext } from 'lib/contexts/multichain';
import dayjs from 'lib/date/dayjs';
import getFilterValueFromQuery from 'lib/getFilterValueFromQuery';
import getFilterValuesFromQuery from 'lib/getFilterValuesFromQuery';
import getValuesArrayFromQuery from 'lib/getValuesArrayFromQuery';
import getQueryParamString from 'lib/router/getQueryParamString';
import { ADVANCED_FILTER_ITEM } from 'stubs/advancedFilter';
import { generateListStub } from 'stubs/utils';
import { Link } from 'toolkit/chakra/link';
import { TableBody, TableCell, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import { Tag } from 'toolkit/chakra/tag';
import ColumnsButton from 'ui/advancedFilter/ColumnsButton';
import type { ColumnsIds } from 'ui/advancedFilter/constants';
import { getAdvancedFilterTypes, TABLE_COLUMNS } from 'ui/advancedFilter/constants';
import ExportCSV from 'ui/advancedFilter/ExportCSV';
import FilterByColumn from 'ui/advancedFilter/FilterByColumn';
import ItemByColumn from 'ui/advancedFilter/ItemByColumn';
import { getDurationFromAge, getFilterTags } from 'ui/advancedFilter/lib';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import IconSvg from 'ui/shared/IconSvg';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

const COLUMNS_CHECKED = {} as Record<ColumnsIds, boolean>;
TABLE_COLUMNS.forEach(c => COLUMNS_CHECKED[c.id] = true);

const AdvancedFilter = () => {
  const router = useRouter();
  const multichainContext = useMultichainContext();

  const [ filters, setFilters ] = React.useState<AdvancedFilterParams>(() => {
    const age = getFilterValueFromQuery(ADVANCED_FILTER_AGES, router.query.age);
    const addressRelation = getFilterValueFromQuery(ADVANCED_FILTER_ADDRESS_RELATION, router.query.address_relation);
    return {
      transaction_types: getFilterValuesFromQuery(
        getAdvancedFilterTypes(multichainContext?.chain?.app_config).map(t => t.id),
        router.query.transaction_types,
      ),
      methods: getValuesArrayFromQuery(router.query.methods),
      methods_names: getValuesArrayFromQuery(router.query.methods_names),
      amount_from: getQueryParamString(router.query.amount_from),
      amount_to: getQueryParamString(router.query.amount_to),
      age,
      age_to: age ? dayjs().toISOString() : getQueryParamString(router.query.age_to),
      age_from: age ? dayjs((dayjs().valueOf() - getDurationFromAge(age))).toISOString() : getQueryParamString(router.query.age_from),
      address_relation: addressRelation,
      token_contract_address_hashes_to_exclude: getValuesArrayFromQuery(router.query.token_contract_address_hashes_to_exclude),
      token_contract_symbols_to_exclude: getValuesArrayFromQuery(router.query.token_contract_symbols_to_exclude),
      token_contract_address_hashes_to_include: getValuesArrayFromQuery(router.query.token_contract_address_hashes_to_include),
      token_contract_symbols_to_include: getValuesArrayFromQuery(router.query.token_contract_symbols_to_include),
      to_address_hashes_to_include: getValuesArrayFromQuery(router.query.to_address_hashes_to_include),
      from_address_hashes_to_include: getValuesArrayFromQuery(router.query.from_address_hashes_to_include),
      to_address_hashes_to_exclude: getValuesArrayFromQuery(router.query.to_address_hashes_to_exclude),
      from_address_hashes_to_exclude: getValuesArrayFromQuery(router.query.from_address_hashes_to_exclude),
    };
  });

  const [ columns, setColumns ] = React.useState<Record<ColumnsIds, boolean>>(COLUMNS_CHECKED);
  const { data, isError, isLoading, pagination, onFilterChange, isPlaceholderData } = useQueryWithPages({
    resourceName: 'general:advanced_filter',
    filters,
    options: {
      placeholderData: generateListStub<'general:advanced_filter'>(
        ADVANCED_FILTER_ITEM,
        50,
        {
          next_page_params: {
            block_number: 5867485,
            internal_transaction_index: 0,
            items_count: 50,
            token_transfer_index: null,
            transaction_index: 2,
          },
          search_params: {
            tokens: {},
            methods: {},
          },
        },
      ),
    },
  });

  // maybe don't need to prefetch, but on dev sepolia those requests take several seconds.
  useApiQuery('general:tokens', { queryParams: { limit: '7', q: '' }, queryOptions: { refetchOnMount: false } });
  useApiQuery('general:advanced_filter_methods', { queryParams: { q: '' }, queryOptions: { refetchOnMount: false } });

  const handleFilterChange = React.useCallback(<T extends keyof AdvancedFilterParams>(field: T, val: AdvancedFilterParams[T]) => {
    setFilters(prevState => {
      const newState = { ...prevState };

      newState[field] = val;
      onFilterChange(newState.age ? omit(newState, [ 'age_from', 'age_to' ]) : newState);
      return newState;
    });
  }, [ onFilterChange ]);

  const onClearFilter = React.useCallback((key: keyof AdvancedFilterParams) => () => {
    if (key === 'methods') {
      handleFilterChange('methods_names', undefined);
    }
    if (key === 'token_contract_address_hashes_to_exclude') {
      handleFilterChange('token_contract_symbols_to_exclude', undefined);
    }
    if (key === 'token_contract_address_hashes_to_include') {
      handleFilterChange('token_contract_symbols_to_include', undefined);
    }
    if (key === 'age') {
      handleFilterChange('age_from', undefined);
      handleFilterChange('age_to', undefined);
    }
    handleFilterChange(key, undefined);
  }, [ handleFilterChange ]);

  const clearAllFilters = React.useCallback(() => {
    setFilters({});
    onFilterChange({});
  }, [ onFilterChange ]);

  const columnsToShow = TABLE_COLUMNS.filter(c => columns[c.id]);

  if (isLoading) {
    return null;
  }

  const filterTags = getFilterTags(filters, multichainContext?.chain?.app_config);

  const content = (
    <AddressHighlightProvider>
      <Box maxW="100%" display="grid" overflowX="scroll" whiteSpace="nowrap">
        <TableRoot tableLayout="fixed" minWidth="950px" w="100%">
          <TableHeaderSticky>
            <TableRow>
              { multichainContext?.chain && <TableColumnHeader width="38px"/> }
              { columnsToShow.map(column => {
                return (
                  <TableColumnHeader
                    key={ column.id }
                    isNumeric={ column.isNumeric }
                    minW={ column.width }
                    w={ column.width }
                    wordBreak="break-word"
                    whiteSpace="normal"
                  >
                    { Boolean(column.name) && (
                      <chakra.span mr={ 2 } lineHeight="24px" verticalAlign="middle">
                        { column.id === 'age' ? 'Timestamp' : column.name }
                      </chakra.span>
                    ) }
                    <FilterByColumn
                      column={ column.id }
                      columnName={ column.name }
                      handleFilterChange={ handleFilterChange }
                      filters={ filters }
                      searchParams={ data?.search_params }
                      isLoading={ isPlaceholderData }
                    />
                    { column.id === 'age' && <TimeFormatToggle ml={ 1 } verticalAlign="middle"/> }
                  </TableColumnHeader>
                );
              }) }
            </TableRow>
          </TableHeaderSticky>
          <TableBody>
            { data?.items.map((item, index) => (
              <TableRow key={ item.hash + String(index) }>
                { multichainContext?.chain && (
                  <TableCell>
                    <ChainIcon data={ multichainContext.chain } isLoading={ isPlaceholderData }/>
                  </TableCell>
                ) }
                { columnsToShow.map(column => {
                  const textAlign = (() => {
                    if (column.id === 'or_and') {
                      return 'center';
                    }
                    if (column.isNumeric) {
                      return 'right';
                    }
                    return 'start';
                  })();

                  return (
                    <TableCell
                      key={ item.hash + column.id }
                      isNumeric={ column.isNumeric }
                      minW={ column.width }
                      maxW={ column.width }
                      w={ column.width }
                      wordBreak="break-word"
                      whiteSpace="nowrap"
                      overflow="hidden"
                      textAlign={ textAlign }
                    >
                      <ItemByColumn
                        item={ item }
                        column={ column.id }
                        isLoading={ isPlaceholderData }
                        chainConfig={ multichainContext?.chain?.app_config }
                      />
                    </TableCell>
                  );
                }) }
              </TableRow>
            )) }
          </TableBody>
        </TableRoot>
      </Box>
    </AddressHighlightProvider>
  );

  const actionBar = (
    <ActionBar mt={ -6 }>
      <ExportCSV filters={ filters }/>
      <ColumnsButton columns={ columns } onChange={ setColumns }/>
      <Pagination ml="auto" { ...pagination }/>
    </ActionBar>
  );

  return (
    <>
      <PageTitle
        title="Advanced filter"
        withTextAd
      />
      <Flex mb={ 4 } justifyContent="space-between" alignItems="start">
        <Text fontSize="lg" mr={ 3 } lineHeight="24px" w="100px">Filtered by:</Text>
        { filterTags.length !== 0 && (
          <Link onClick={ clearAllFilters } display="flex" alignItems="center" justifyContent="end" gap={ 2 } fontSize="sm" w="150px">
            <IconSvg name="repeat" boxSize={ 5 }/>
            Reset filters
          </Link>
        ) }
      </Flex>
      <HStack gap={ 2 } flexWrap="wrap" mb={ 6 }>
        { multichainContext?.chain && (
          <Tag variant="filter" label="Chain">
            { multichainContext.chain.app_config.chain.name }
          </Tag>
        ) }
        { filterTags.map(t => (
          <Tag key={ t.name } variant="filter" onClose={ onClearFilter(t.key) } closable label={ t.name }>
            { t.value }
          </Tag>
        )) }
        { filterTags.length === 0 && (
          <>
            <Tag variant="filter" label="Type">
              All
            </Tag>
            <Tag variant="filter" label="Age">
              7d
            </Tag>
          </>
        ) }
      </HStack>
      <DataListDisplay
        isError={ isError }
        itemsNum={ data?.items.length }
        emptyText="There are no transactions."
        actionBar={ actionBar }
        hasActiveFilters={ Object.values(filters).some(Boolean) }
        emptyStateProps={{
          term: 'transaction',
        }}
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default AdvancedFilter;
