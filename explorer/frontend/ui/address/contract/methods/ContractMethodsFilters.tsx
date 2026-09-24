import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { MethodType } from './types';

import { ButtonGroupRadio, Button } from 'toolkit/chakra/button';
import { FilterInput } from 'toolkit/components/filters/FilterInput';

import type { MethodsFilters } from './useMethodsFilters';
import { TYPE_FILTER_OPTIONS } from './utils';

interface Props {
  defaultMethodType: MethodType;
  defaultSearchTerm: string;
  onChange: (filter: MethodsFilters) => void;
  isLoading?: boolean;
}

const ContractMethodsFilters = ({ defaultMethodType, defaultSearchTerm, onChange, isLoading }: Props) => {

  const handleTypeChange = React.useCallback((value: string) => {
    onChange({ type: 'method_type', value: value as MethodType });
  }, [ onChange ]);

  const handleSearchTermChange = React.useCallback((value: string) => {
    onChange({ type: 'method_name', value });
  }, [ onChange ]);

  return (
    <Flex columnGap={ 3 } rowGap={ 3 } flexDir={{ base: 'column', lg: 'row' }}>
      <ButtonGroupRadio
        defaultValue={ defaultMethodType }
        onChange={ handleTypeChange }
        w={{ lg: 'fit-content' }}
        loading={ isLoading }
      >
        { TYPE_FILTER_OPTIONS.map((option) => (
          <Button key={ option.value } value={ option.value } size="sm" px={ 3 }>
            { option.title }
          </Button>
        )) }
      </ButtonGroupRadio>
      <FilterInput
        initialValue={ defaultSearchTerm }
        onChange={ handleSearchTermChange }
        placeholder="Search by method name"
        w={{ base: '100%', lg: '450px' }}
        size="sm"
        loading={ isLoading }
      />
    </Flex>
  );
};

export default React.memo(ContractMethodsFilters);
