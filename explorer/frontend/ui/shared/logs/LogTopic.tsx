import { createListCollection, Flex } from '@chakra-ui/react';
import { capitalize } from 'es-toolkit';
import React from 'react';

import hexToAddress from 'lib/hexToAddress';
import hexToUtf8 from 'lib/hexToUtf8';
import { SelectContent, SelectControl, SelectItem, SelectRoot, SelectValueText } from 'toolkit/chakra/select';
import { Skeleton } from 'toolkit/chakra/skeleton';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';

import LogIndex from './LogIndex';

interface Props {
  hex: string;
  index: number;
  isLoading?: boolean;
}

type DataType = 'hex' | 'text' | 'address' | 'number';

const VALUE_CONVERTERS: Record<DataType, (hex: string) => string> = {
  hex: (hex) => hex,
  text: hexToUtf8,
  address: hexToAddress,
  number: (hex) => BigInt(hex).toString(),
};
const OPTIONS: Array<DataType> = [ 'hex', 'address', 'text', 'number' ];

const collection = createListCollection({
  items: OPTIONS.map((option) => ({
    value: option,
    label: capitalize(option),
  })),
});

const LogTopic = ({ hex, index, isLoading }: Props) => {
  const [ selectedDataType, setSelectedDataType ] = React.useState<DataType>('hex');

  const handleSelectChange = React.useCallback((details: { value: Array<string> }) => {
    setSelectedDataType(details.value[0] as DataType);
  }, []);

  const value = VALUE_CONVERTERS[selectedDataType.toLowerCase() as Lowercase<DataType>](hex);

  const content = (() => {
    switch (selectedDataType) {
      case 'hex':
      case 'number':
      case 'text': {
        return (
          <>
            <Skeleton loading={ isLoading } overflow="hidden" whiteSpace="nowrap">
              <HashStringShortenDynamic hash={ value }/>
            </Skeleton>
            <CopyToClipboard text={ value } isLoading={ isLoading }/>
          </>
        );
      }

      case 'address': {
        return (
          <AddressEntity
            address={{ hash: value, name: '' }}
            isLoading={ isLoading }
          />
        );
      }
    }
  })();

  return (
    <Flex alignItems="center" px={{ base: 0, lg: 3 }} _notFirst={{ mt: 3 }} overflow="hidden" maxW="100%">
      <LogIndex
        isLoading={ isLoading }
        textStyle="xs"
        mr={ 3 }
        minW={ 6 }
        height={ 6 }
      >
        { index }
      </LogIndex>
      { index !== 0 && (
        <SelectRoot
          collection={ collection }
          variant="outline"
          value={ [ selectedDataType ] }
          onValueChange={ handleSelectChange }
          mr={ 3 }
          flexShrink={ 0 }
          width="fit-content"
        >
          <SelectControl w="105px" loading={ isLoading }>
            <SelectValueText placeholder="Data type"/>
          </SelectControl>
          <SelectContent>
            { collection.items.map((item) => (
              <SelectItem item={ item } key={ item.value }>
                { item.label }
              </SelectItem>
            )) }
          </SelectContent>
        </SelectRoot>
      ) }
      { content }
    </Flex>
  );
};

export default React.memo(LogTopic);
