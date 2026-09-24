import { Box } from '@chakra-ui/react';
import React from 'react';

import type { SmartContract } from 'types/api/contract';

import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import type { Truncation } from 'ui/shared/entities/base/components';
import RawDataSnippet from 'ui/shared/RawDataSnippet';

import { matchArray } from './methods/form/utils';

interface DecodedItemProps {
  value: unknown;
  type: string;
  addressTruncation?: Truncation;
}

const DecodedItemValue = ({ value, type, addressTruncation = 'dynamic' }: DecodedItemProps) => {
  const arrayMatch = matchArray(type);

  if (arrayMatch && Array.isArray(value)) {
    return value.map((item, index) => (
      <>
        <DecodedItemValue key={ index } value={ item } type={ arrayMatch.itemType } addressTruncation="constant"/>
        { index < value.length - 1 && ', ' }
      </>
    ));
  }

  if (type === 'address' && typeof value === 'string') {
    return (
      <AddressEntity
        address={{ hash: value }}
        noIcon
        display="inline-flex"
        maxW="100%"
        truncation={ addressTruncation }
      />
    );
  }

  const content = (() => {
    if (value === null || value === undefined || value === '') {
      return '""';
    }

    if (typeof value === 'object') {
      return JSON.stringify(value);
    }

    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
      return value;
    }

    return String(value);
  })();

  return <span>{ content }</span>;
};

interface Props {
  data: SmartContract | undefined;
  isLoading: boolean;
}

const ContractDetailsConstructorArgs = ({ data, isLoading }: Props) => {

  const content = React.useMemo(() => {
    if (!data?.decoded_constructor_args) {
      return data?.constructor_args;
    }

    const decoded = data.decoded_constructor_args
      .map(([ value, { name, type } ], index) => {
        return (
          <Box key={ index }>
            <span>Arg [{ index }] { name || '' } ({ type }): </span>
            <DecodedItemValue value={ value } type={ type }/>
          </Box>
        );
      });

    return (
      <>
        <span>{ data.constructor_args }</span>
        <br/><br/>
        { decoded }
      </>
    );
  }, [ data?.constructor_args, data?.decoded_constructor_args ]);

  if (!content) {
    return null;
  }

  return (
    <RawDataSnippet
      data={ content }
      title="Constructor Arguments"
      textareaMaxHeight="200px"
      isLoading={ isLoading }
    />
  );
};

export default React.memo(ContractDetailsConstructorArgs);
