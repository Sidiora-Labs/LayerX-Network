import { Flex, Text, Spinner, createListCollection } from '@chakra-ui/react';
import { isEqual } from 'es-toolkit';
import React from 'react';

import type { AdvancedFilterParams } from 'types/api/advancedFilter';
import type { TokenInfo } from 'types/api/token';

import useApiQuery from 'lib/api/useApiQuery';
import useDebounce from 'lib/hooks/useDebounce';
import { Checkbox, CheckboxGroup } from 'toolkit/chakra/checkbox';
import { Select } from 'toolkit/chakra/select';
import { Tag } from 'toolkit/chakra/tag';
import { ClearButton } from 'toolkit/components/buttons/ClearButton';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import * as TokenEntity from 'ui/shared/entities/token/TokenEntity';
import TableColumnFilter from 'ui/shared/filters/TableColumnFilter';
import NativeTokenIcon from 'ui/shared/NativeTokenIcon';

import { NATIVE_TOKEN } from '../constants';

const FILTER_PARAM_INCLUDE = 'token_contract_address_hashes_to_include';
const FILTER_PARAM_EXCLUDE = 'token_contract_address_hashes_to_exclude';
const NAME_PARAM_INCLUDE = 'token_contract_symbols_to_include';
const NAME_PARAM_EXCLUDE = 'token_contract_symbols_to_exclude';

export type AssetFilterMode = 'include' | 'exclude';

const collection = createListCollection({
  items: [
    { label: 'Include', value: 'include' },
    { label: 'Exclude', value: 'exclude' },
  ],
});

// add native token
type Value = Array<{ token: TokenInfo; mode: AssetFilterMode }>;

type Props = {
  value: Value;
  handleFilterChange: (filed: keyof AdvancedFilterParams, val: Array<string>) => void;
  columnName: string;
  isLoading?: boolean;
};

const AssetFilter = ({ value = [], handleFilterChange }: Props) => {
  const [ currentValue, setCurrentValue ] = React.useState<Value>([ ...value ]);
  const [ searchTerm, setSearchTerm ] = React.useState<string>('');
  const debouncedSearchTerm = useDebounce(searchTerm, 300);

  const onSearchChange = React.useCallback((value: string) => {
    setSearchTerm(value);
  }, []);

  const handleModeSelectChange = React.useCallback((index: number) => ({ value }: { value: Array<string> }) => {
    setCurrentValue(prev => {
      const newValue = [ ...prev ];
      newValue[index] = { ...prev[index], mode: value[0] as AssetFilterMode };
      return newValue;
    });
  }, []);

  const handleRemove = React.useCallback((index: number) => () => {
    setCurrentValue(prev => {
      prev.splice(index, 1);
      return [ ...prev ];
    });
  }, []);

  const tokensQuery = useApiQuery('general:tokens', {
    queryParams: { limit: debouncedSearchTerm ? undefined : '7', q: debouncedSearchTerm },
    queryOptions: {
      refetchOnMount: false,
    },
  });

  const onTokenClick = React.useCallback((token: TokenInfo) => () => {
    setCurrentValue(prev => prev.findIndex(i => i.token.address_hash === token.address_hash) > -1 ? prev : [ { token, mode: 'include' }, ...prev ]);
  }, []);

  const onReset = React.useCallback(() => setCurrentValue([]), []);

  const onFilter = React.useCallback(() => {
    setSearchTerm('');
    handleFilterChange(FILTER_PARAM_INCLUDE, currentValue.filter(i => i.mode === 'include').map(i => i.token.address_hash));
    handleFilterChange(NAME_PARAM_INCLUDE, currentValue.filter(i => i.mode === 'include').map(i => i.token.symbol || ''));
    handleFilterChange(FILTER_PARAM_EXCLUDE, currentValue.filter(i => i.mode === 'exclude').map(i => i.token.address_hash));
    handleFilterChange(NAME_PARAM_EXCLUDE, currentValue.filter(i => i.mode === 'exclude').map(i => i.token.symbol || ''));
    return;
  }, [ handleFilterChange, currentValue ]);

  return (
    <TableColumnFilter
      title="Asset"
      isFilled={ Boolean(currentValue.length) }
      isTouched={ !isEqual(currentValue.map(i => JSON.stringify(i)).sort(), value.map(i => JSON.stringify(i)).sort()) }
      onFilter={ onFilter }
      onReset={ onReset }
      hasReset
    >
      <FilterInput
        size="sm"
        onChange={ onSearchChange }
        placeholder="Token name or symbol"
        initialValue={ searchTerm }
      />
      { !searchTerm && currentValue.map((item, index) => (
        <Flex key={ item.token.address_hash } alignItems="center">
          <Select
            collection={ collection }
            placeholder="Select mode"
            defaultValue={ [ item.mode || 'include' ] }
            onValueChange={ handleModeSelectChange(index) }
            portalled={ false }
            w="105px"
            minW="105px"
            mr={ 3 }
          />
          <TokenEntity.default token={ item.token } noLink noCopy flexGrow={ 1 }/>
          <ClearButton onClick={ handleRemove(index) }/>
        </Flex>
      )) }
      { tokensQuery.isLoading && <Spinner display="block" mt={ 3 }/> }
      { tokensQuery.data && !searchTerm && (
        <>
          <Text color="text.secondary" fontWeight="600" mt={ 3 }>Popular</Text>
          <Flex rowGap={ 3 } flexWrap="wrap" gap={ 3 } mb={ 2 }>
            { [ NATIVE_TOKEN, ...tokensQuery.data.items ].map(token => (
              <Tag
                key={ token.address_hash }
                data-id={ token.address_hash }
                onClick={ onTokenClick(token) }
                variant="select"
              >
                <Flex flexGrow={ 1 } alignItems="center">
                  { token.address_hash === NATIVE_TOKEN.address_hash ? <NativeTokenIcon boxSize={ 5 } mr={ 2 }/> : <TokenEntity.Icon token={ token }/> }
                  { token.symbol || token.name || token.address_hash }
                </Flex>
              </Tag>
            )) }
          </Flex>
        </>
      ) }
      { searchTerm && tokensQuery.data && !tokensQuery.data?.items.length && <Text>No tokens found</Text> }
      { searchTerm && tokensQuery.data && Boolean(tokensQuery.data?.items.length) && (
        <Flex display="flex" flexDir="column" rowGap={ 3 } maxH="250px" overflowY="scroll" mt={ 3 } ml="-4px">
          <CheckboxGroup value={ currentValue.map(i => i.token.address_hash) } orientation="vertical">
            { tokensQuery.data.items.map(token => (
              <Checkbox
                key={ token.address_hash }
                value={ token.address_hash }
                id={ token.address_hash }
                onChange={ onTokenClick(token) }
                overflow="hidden"
                w="100%"
                pl={ 1 }
              >
                <TokenEntity.default token={ token } noLink noCopy/>
              </Checkbox>
            )) }
          </CheckboxGroup>
        </Flex>
      ) }
    </TableColumnFilter>
  );
};

export default AssetFilter;
