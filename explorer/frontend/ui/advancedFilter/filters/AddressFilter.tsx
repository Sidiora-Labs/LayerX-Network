import { createListCollection, Flex, VStack } from '@chakra-ui/react';
import { isEqual } from 'es-toolkit';
import type { ChangeEvent } from 'react';
import React from 'react';

import type { AdvancedFilterParams } from 'types/api/advancedFilter';

import { Field } from 'toolkit/chakra/field';
import { Input } from 'toolkit/chakra/input';
import { InputGroup } from 'toolkit/chakra/input-group';
import { Select } from 'toolkit/chakra/select';
import AddButton from 'toolkit/components/buttons/AddButton';
import { ClearButton } from 'toolkit/components/buttons/ClearButton';
import { ADDRESS_REGEXP } from 'toolkit/utils/regexp';
import TableColumnFilter from 'ui/shared/filters/TableColumnFilter';

const FILTER_PARAM_TO_INCLUDE = 'to_address_hashes_to_include';
const FILTER_PARAM_FROM_INCLUDE = 'from_address_hashes_to_include';
const FILTER_PARAM_TO_EXCLUDE = 'to_address_hashes_to_exclude';
const FILTER_PARAM_FROM_EXCLUDE = 'from_address_hashes_to_exclude';

export type AddressFilterMode = 'include' | 'exclude';

const collection = createListCollection({
  items: [
    { label: 'Include', value: 'include' },
    { label: 'Exclude', value: 'exclude' },
  ],
});

type Value = Array<{ address: string; mode: AddressFilterMode }>;

type Props = {
  value: Value;
  handleFilterChange: (filed: keyof AdvancedFilterParams, val: Array<string> | undefined) => void;
  columnName: string;
  type: 'from' | 'to';
  isLoading?: boolean;
};

type InputProps = {
  address?: string;
  mode?: AddressFilterMode;
  isLast: boolean;
  onModeChange: ({ value }: { value: Array<string> }) => void;
  onChange: (event: ChangeEvent<HTMLInputElement>) => void;
  onBlur: () => void;
  onClear: () => void;
  onAddFieldClick: () => void;
  isInvalid: boolean;
};

type AddressFilter = {
  address: string;
  mode: AddressFilterMode;
};

function addressFilterToKey(filter: AddressFilter) {
  return `${ filter.address.toLowerCase() }-${ filter.mode }`;
}

const AddressFilterInput = ({ address, mode, onModeChange, onChange, onBlur, onClear, isLast, onAddFieldClick, isInvalid }: InputProps) => {
  return (
    <Flex alignItems="flex-start" w="100%">
      <Select
        collection={ collection }
        placeholder="Select mode"
        defaultValue={ [ mode || 'include' ] }
        onValueChange={ onModeChange }
        portalled={ false }
        w="105px"
        flexShrink={ 0 }
        mr={ 3 }
      />
      <Field
        flexGrow={ 1 }
        invalid={ isInvalid }
        errorText="Invalid address format"
      >
        <InputGroup
          endElement={ <ClearButton onClick={ onClear } mx={ 2 } disabled={ !address }/> }
        >
          <Input value={ address } onChange={ onChange } onBlur={ onBlur } placeholder="Smart contract / Address (0x...)*" size="sm" autoComplete="off"/>
        </InputGroup>
      </Field>
      { isLast && (
        <AddButton
          ml={ 2 }
          onClick={ onAddFieldClick }
        />
      ) }
    </Flex>
  );
};

const emptyItem = { address: '', mode: 'include' as AddressFilterMode };

const AddressFilter = ({ type, value = [], handleFilterChange }: Props) => {
  const [ currentValue, setCurrentValue ] =
    React.useState<Array<AddressFilter>>([ ...value, emptyItem ]);
  const [ touched, setTouched ] = React.useState<Array<boolean>>(value.map(() => true).concat(false));

  const handleModeSelectChange = React.useCallback((index: number) => ({ value }: { value: Array<string> }) => {
    setCurrentValue(prev => {
      prev[index] = { ...prev[index], mode: value[0] as AddressFilterMode };
      return [ ...prev ];
    });
  }, []);

  const handleAddressClear = React.useCallback((index: number) => () => {
    setCurrentValue(prev => {
      const newVal = [ ...prev ];
      newVal[index] = { ...newVal[index], address: '' };
      return newVal;
    });
    setTouched(prev => {
      const newTouched = [ ...prev ];
      newTouched[index] = false;
      return newTouched;
    });
  }, []);

  const handleAddressChange = React.useCallback((index: number) => (event: React.ChangeEvent<HTMLInputElement>) => {
    const value = event.target.value;

    setCurrentValue(prev => {
      const newVal = [ ...prev ];
      newVal[index] = { ...newVal[index], address: value };
      return newVal;
    });
  }, []);

  const handleAddressBlur = React.useCallback((index: number) => () => {
    setTouched(prev => {
      const newTouched = [ ...prev ];
      newTouched[index] = true;
      return newTouched;
    });
  }, []);

  const onAddFieldClick = React.useCallback(() => {
    setCurrentValue(prev => [ ...prev, emptyItem ]);
    setTouched(prev => [ ...prev, false ]);
  }, []);

  const onReset = React.useCallback(() => {
    setCurrentValue([ emptyItem ]);
    setTouched([ false ]);
  }, []);

  const onFilter = React.useCallback(() => {
    const includeFilterParam = type === 'from' ? FILTER_PARAM_FROM_INCLUDE : FILTER_PARAM_TO_INCLUDE;
    const excludeFilterParam = type === 'from' ? FILTER_PARAM_FROM_EXCLUDE : FILTER_PARAM_TO_EXCLUDE;
    const includeValue = currentValue.filter(i => i.mode === 'include').map(i => i.address).filter(Boolean);
    const excludeValue = currentValue.filter(i => i.mode === 'exclude').map(i => i.address).filter(Boolean);

    handleFilterChange(includeFilterParam, includeValue.length ? includeValue : undefined);
    handleFilterChange(excludeFilterParam, excludeValue.length ? excludeValue : undefined);
  }, [ handleFilterChange, currentValue, type ]);

  const hasErrors = currentValue.some(i => Boolean(i.address) && !ADDRESS_REGEXP.test(i.address));
  const isTouched = !isEqual(currentValue.filter(i => i.address).map(addressFilterToKey).sort(), value.map(addressFilterToKey).sort());

  return (
    <TableColumnFilter
      title={ type === 'from' ? 'From address' : 'To address' }
      isFilled={ Boolean(currentValue[0].address) }
      isTouched={ isTouched && !hasErrors }
      onFilter={ onFilter }
      onReset={ onReset }
      hasReset
    >
      <VStack gap={ 2 }>
        { currentValue.map((item, index) => (
          <AddressFilterInput
            key={ index }
            address={ item.address }
            mode={ item.mode }
            isLast={ index === currentValue.length - 1 }
            onModeChange={ handleModeSelectChange(index) }
            onChange={ handleAddressChange(index) }
            onBlur={ handleAddressBlur(index) }
            onClear={ handleAddressClear(index) }
            onAddFieldClick={ onAddFieldClick }
            isInvalid={ Boolean(touched[index]) && Boolean(item.address) && !ADDRESS_REGEXP.test(item.address) }
          />
        )) }
      </VStack>
    </TableColumnFilter>
  );
};

export default AddressFilter;
