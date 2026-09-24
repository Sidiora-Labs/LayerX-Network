import { chakra } from '@chakra-ui/react';
import React from 'react';
import type { AbiParameter } from 'viem';

interface Props {
  abiParameter: AbiParameter;
}

const ItemLabel = ({ abiParameter }: Props) => {
  return (
    <>
      { abiParameter.name && <chakra.span fontWeight={ 500 }>{ abiParameter.name } </chakra.span> }
      <span>({ abiParameter.type }) : </span>
    </>
  );
};

export default React.memo(ItemLabel);
