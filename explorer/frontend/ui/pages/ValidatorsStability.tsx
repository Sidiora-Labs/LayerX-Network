import { Box, createListCollection, HStack } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type {
  ValidatorsStabilityFilters,
  ValidatorsStabilitySorting,
  ValidatorsStabilitySortingField,
  ValidatorsStabilitySortingValue,
} from 'types/api/validators';

import config from 'configs/app';
// import useDebounce from 'lib/hooks/useDebounce';
import useIsMobile from 'lib/hooks/useIsMobile';
import getQueryParamString from 'lib/router/getQueryParamString';
import { generateListStub } from 'stubs/utils';
import { VALIDATOR_STABILITY } from 'stubs/validators';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
// import { FilterInput } from 'toolkit/components/filters/FilterInput';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import getSortParamsFromValue from 'ui/shared/sort/getSortParamsFromValue';
import getSortValueFromQuery from 'ui/shared/sort/getSortValueFromQuery';
import Sort from 'ui/shared/sort/Sort';
import { VALIDATORS_STABILITY_SORT_OPTIONS } from 'ui/validators/stability/utils';
import ValidatorsCounters from 'ui/validators/stability/ValidatorsCounters';
import ValidatorsFilter from 'ui/validators/stability/ValidatorsFilter';
import ValidatorsList from 'ui/validators/stability/ValidatorsList';
import ValidatorsTable from 'ui/validators/stability/ValidatorsTable';

const sortCollection = createListCollection({
  items: VALIDATORS_STABILITY_SORT_OPTIONS,
});

const ValidatorsStability = () => {
  const router = useRouter();
  // const [ searchTerm, setSearchTerm ] = React.useState(getQueryParamString(router.query.address_hash) || undefined);
  const [ statusFilter, setStatusFilter ] =
    React.useState(getQueryParamString(router.query.state_filter) as ValidatorsStabilityFilters['state_filter'] || undefined);
  const [ sort, setSort ] = React.useState<ValidatorsStabilitySortingValue>(
    getSortValueFromQuery<ValidatorsStabilitySortingValue>(router.query, VALIDATORS_STABILITY_SORT_OPTIONS) ?? 'default',
  );

  // const debouncedSearchTerm = useDebounce(searchTerm || '', 300);

  const isMobile = useIsMobile();

  const { isError, isPlaceholderData, data, pagination, onFilterChange, onSortingChange } = useQueryWithPages({
    resourceName: 'general:validators_stability',
    filters: {
      // address_hash: debouncedSearchTerm,
      state_filter: statusFilter,
    },
    sorting: getSortParamsFromValue<ValidatorsStabilitySortingValue, ValidatorsStabilitySortingField, ValidatorsStabilitySorting['order']>(sort),
    options: {
      enabled: config.features.validators.isEnabled,
      placeholderData: generateListStub<'general:validators_stability'>(
        VALIDATOR_STABILITY,
        50,
        { next_page_params: null },
      ),
    },
  });

  // const handleSearchTermChange = React.useCallback((value: string) => {
  //   onFilterChange({
  //     address_hash: value,
  //     state_filter: statusFilter
  //   });
  //   setSearchTerm(value);
  // }, [ statusFilter, onFilterChange ]);

  const handleStateFilterChange = React.useCallback((value: string | Array<string>) => {
    if (Array.isArray(value)) {
      return;
    }

    const state = value === 'all' ? undefined : value as ValidatorsStabilityFilters['state_filter'];

    onFilterChange({
      // address_hash: debouncedSearchTerm,
      state_filter: state,
    });
    setStatusFilter(state);
  }, [ onFilterChange ]);

  const handleSortChange = React.useCallback(({ value }: { value: Array<string> }) => {
    const sortValue = value[0] as ValidatorsStabilitySortingValue;
    setSort(sortValue);
    onSortingChange(sortValue === 'default' ? undefined : getSortParamsFromValue(sortValue));
  }, [ onSortingChange ]);

  const filterMenu =
    <ValidatorsFilter onChange={ handleStateFilterChange } defaultValue={ statusFilter } hasActiveFilter={ Boolean(statusFilter) }/>;

  // const filterInput = (
  //   <FilterInput
  //     w={{ base: '100%', lg: '350px' }}
  //     size="xs"
  //     onChange={ handleSearchTermChange }
  //     placeholder="Search by validator's address hash"
  //     initialValue={ searchTerm }
  //   />
  // );

  const sortButton = (
    <Sort
      name="validators_sorting"
      defaultValue={ [ sort ] }
      collection={ sortCollection }
      onValueChange={ handleSortChange }
    />
  );

  const actionBar = (
    <>
      <HStack gap={ 3 } mb={ 6 } display={{ base: 'flex', lg: 'none' }}>
        { filterMenu }
        { sortButton }
        { /* { filterInput } */ }
      </HStack>
      { (!isMobile || pagination.isVisible) && (
        <ActionBar mt={ -6 }>
          <HStack gap={ 3 } display={{ base: 'none', lg: 'flex' }}>
            { filterMenu }
            { /* { filterInput } */ }
          </HStack>
          <Pagination ml="auto" { ...pagination }/>
        </ActionBar>
      ) }
    </>
  );

  const content = data?.items ? (
    <>
      <Box hideFrom="lg">
        <ValidatorsList data={ data.items } isLoading={ isPlaceholderData }/>
      </Box>
      <Box hideBelow="lg">
        <ValidatorsTable data={ data.items } sort={ sort } setSorting={ handleSortChange } isLoading={ isPlaceholderData }/>
      </Box>
    </>
  ) : null;

  return (
    <Box>
      <PageTitle title="Validators" withTextAd/>
      <ValidatorsCounters/>
      <DataListDisplay
        isError={ isError }
        itemsNum={ data?.items.length }
        emptyText="There are no validators."
        hasActiveFilters={ Boolean(
          // searchTerm ||
          statusFilter,
        ) }
        emptyStateProps={{
          term: 'validator',
        }}
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </Box>
  );
};

export default ValidatorsStability;
