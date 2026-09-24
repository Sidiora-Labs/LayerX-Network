import { chakra } from '@chakra-ui/react';
import React from 'react';
import type { AbiParameter } from 'viem';

import type { ResultViewMode } from '../../types';

import { matchArray, type MatchArray } from '../utils';
import ItemLabel from './ItemLabel';
import ItemPrimitive from './ItemPrimitive';
import ItemTuple from './ItemTuple';
import { printRowOffset } from './utils';

interface Props {
  abiParameter: AbiParameter;
  data: Array<unknown>;
  mode: ResultViewMode;
  level: number;
  arrayMatch: MatchArray;
}

const ItemArray = ({ abiParameter, data, level, arrayMatch, mode }: Props) => {

  const itemAbiParameter = React.useMemo(() => {
    const type = arrayMatch.itemType;
    const internalType = matchArray(abiParameter.internalType || '')?.itemType;
    return { ...abiParameter, type, internalType };
  }, [ abiParameter, arrayMatch.itemType ]);

  const content = (() => {
    if (arrayMatch.isNested && data.every(Array.isArray)) {
      const itemArrayMatch = matchArray(arrayMatch.itemType);

      if (itemArrayMatch) {
        return data.map((item, index) => (
          <ItemArray
            key={ index }
            abiParameter={ itemAbiParameter }
            data={ item }
            mode={ mode }
            arrayMatch={ itemArrayMatch }
            level={ level + 1 }
          />
        ));
      }
    }

    if (arrayMatch.itemType === 'tuple') {
      return data.map((item, index) => (
        <ItemTuple
          key={ index }
          abiParameter={ itemAbiParameter }
          data={ item }
          mode={ mode }
          level={ level + 1 }
        />
      ));
    }

    return data.map((item, index) => (
      <ItemPrimitive
        key={ index }
        abiParameter={ itemAbiParameter }
        data={ item }
        level={ level + 1 }
        hideLabel
      />
    ));
  })();

  return (
    <chakra.span display="block">
      <chakra.span display="block">
        <span>{ printRowOffset(level) }</span>
        <ItemLabel abiParameter={ abiParameter }/>
        <span>[{ data.length === 0 ? ' ]' : '' }</span>
      </chakra.span>
      { content }
      { data.length > 0 && <p>{ printRowOffset(level) }]</p> }
    </chakra.span>
  );
};

export default React.memo(ItemArray);
