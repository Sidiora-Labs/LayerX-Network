import { Box, Fieldset, Flex, HStack, Text, chakra, createListCollection } from '@chakra-ui/react';
import React from 'react';

import type * as bens from '@blockscout/bens-types';
import type { EnsDomainLookupFiltersOptions } from 'types/api/ens';
import type { PaginationParams } from 'ui/shared/pagination/types';

import useIsInitialLoading from 'lib/hooks/useIsInitialLoading';
import { Button } from 'toolkit/chakra/button';
import { Checkbox, CheckboxGroup } from 'toolkit/chakra/checkbox';
import { Image } from 'toolkit/chakra/image';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import ActionBar from 'ui/shared/ActionBar';
import PopoverFilter from 'ui/shared/filters/PopoverFilter';
import IconSvg from 'ui/shared/IconSvg';
import Pagination from 'ui/shared/pagination/Pagination';
import Sort from 'ui/shared/sort/Sort';

import type { Sort as TSort } from './utils';
import { SORT_OPTIONS } from './utils';

const sortCollection = createListCollection({ items: SORT_OPTIONS });

interface Props {
  pagination: PaginationParams;
  searchTerm: string | undefined;
  onSearchChange: (value: string) => void;
  filterValue: EnsDomainLookupFiltersOptions;
  onFilterValueChange: (nextValue: EnsDomainLookupFiltersOptions) => void;
  protocolsData: Array<bens.ProtocolInfo> | undefined;
  protocolsFilterValue: Array<string>;
  onProtocolsFilterChange: (nextValue: Array<string>) => void;
  sort: TSort;
  onSortChange: (nextValue: TSort) => void;
  isLoading: boolean;
  isAddressSearch: boolean;
}

const NameDomainsActionBar = ({
  searchTerm,
  onSearchChange,
  filterValue,
  onFilterValueChange,
  sort,
  onSortChange,
  isLoading,
  isAddressSearch,
  pagination,
  protocolsData,
  protocolsFilterValue,
  onProtocolsFilterChange,
}: Props) => {
  const isInitialLoading = useIsInitialLoading(isLoading);

  const searchInput = (
    <FilterInput
      w={{ base: '100%', lg: '360px' }}
      minW={{ base: 'auto', lg: '250px' }}
      size="sm"
      onChange={ onSearchChange }
      placeholder="Search by name or address"
      initialValue={ searchTerm }
      loading={ isInitialLoading }
    />
  );

  const handleProtocolReset = React.useCallback(() => {
    onProtocolsFilterChange([]);
  }, [ onProtocolsFilterChange ]);

  const handleSortChange = React.useCallback(({ value }: { value: Array<string> }) => {
    onSortChange(value[0] as TSort);
  }, [ onSortChange ]);

  const handleFilterValueChange = React.useCallback((value: Array<string>) => {
    onFilterValueChange(value as EnsDomainLookupFiltersOptions);
  }, [ onFilterValueChange ]);

  const filterGroupDivider = <Box w="100%" borderBottomWidth="1px" borderBottomColor="border.divider" my={ 4 }/>;

  const appliedFiltersNum = filterValue.length + (protocolsData && protocolsData.length > 1 ? protocolsFilterValue.length : 0);

  const filter = (
    <PopoverFilter appliedFiltersNum={ appliedFiltersNum } contentProps={{ minW: '220px', w: 'fit-content' }} isLoading={ isInitialLoading }>
      <div>
        { protocolsData && protocolsData.length > 1 && (
          <>
            <Flex justifyContent="space-between" textStyle="sm" mb={ 3 }>
              <Text fontWeight={ 600 } color="text.secondary">Protocol</Text>
              <Button
                variant="link"
                onClick={ handleProtocolReset }
                disabled={ protocolsFilterValue.length === 0 }
                textStyle="sm"
              >
                Reset
              </Button>
            </Flex>
            <CheckboxGroup defaultValue={ protocolsFilterValue } onValueChange={ onProtocolsFilterChange } value={ protocolsFilterValue } name="token_type">
              { protocolsData.map((protocol) => {
                const topLevelDomains = protocol.tld_list.map((domain) => `.${ domain }`).join(' ');
                return (
                  <Checkbox key={ protocol.id } value={ protocol.id }>
                    <Flex alignItems="center">
                      <Image
                        src={ protocol.icon_url }
                        boxSize={ 5 }
                        borderRadius="sm"
                        mr={ 2 }
                        alt={ `${ protocol.title } protocol icon` }
                        fallback={ <IconSvg name="ENS"/> }
                      />
                      <span>{ protocol.short_name }</span>
                      <chakra.span color="text.secondary" whiteSpace="pre"> { topLevelDomains }</chakra.span>
                    </Flex>
                  </Checkbox>
                );
              }) }
            </CheckboxGroup>
            { filterGroupDivider }
          </>
        ) }
        <Fieldset.Root>
          <CheckboxGroup defaultValue={ filterValue } onValueChange={ handleFilterValueChange } value={ filterValue } name="token_type">
            <Fieldset.Content gap={ 0 }>
              <Text color="text.secondary" fontWeight={ 600 } mb={ 3 } textStyle="sm">Address</Text>
              <Checkbox value="owned_by" disabled={ !isAddressSearch }>
                Owned by
              </Checkbox>
              <Checkbox
                value="resolved_to"
                mt={ 3 }
                disabled={ !isAddressSearch }
              >
                Resolved to address
              </Checkbox>
              { filterGroupDivider }
              <Text color="text.secondary" fontWeight={ 600 } mb={ 3 } textStyle="sm">Status</Text>
              <Checkbox value="with_inactive">
                Include expired
              </Checkbox>
            </Fieldset.Content>
          </CheckboxGroup>
        </Fieldset.Root>
      </div>
    </PopoverFilter>
  );

  const sortButton = (
    <Sort
      name="name_domains_sorting"
      defaultValue={ [ sort ] }
      collection={ sortCollection }
      onValueChange={ handleSortChange }
      isLoading={ isInitialLoading }
    />
  );

  return (
    <>
      <HStack gap={ 3 } mb={ 6 } hideFrom="lg">
        { filter }
        { sortButton }
        { searchInput }
      </HStack>
      <ActionBar
        mt={ -6 }
        display={{ base: pagination.isVisible ? 'flex' : 'none', lg: 'flex' }}
      >
        <HStack gap={ 3 } hideBelow="lg">
          { filter }
          { searchInput }
        </HStack>
        <Pagination { ...pagination } ml="auto"/>
      </ActionBar>
    </>
  );
};

export default React.memo(NameDomainsActionBar);
