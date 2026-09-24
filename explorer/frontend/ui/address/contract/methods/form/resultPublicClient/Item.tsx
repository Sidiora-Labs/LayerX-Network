import React from 'react';
import type { AbiParameter } from 'viem';

import type { ResultViewMode } from '../../types';

import { matchArray } from '../utils';
import ItemArray from './ItemArray';
import ItemPrimitive from './ItemPrimitive';
import ItemTuple from './ItemTuple';

interface Props {
  abiParameter: AbiParameter;
  data: unknown;
  mode: ResultViewMode;
  level?: number;
}

const Item = ({ abiParameter, data, mode, level = 0 }: Props) => {

  const arrayMatch = React.useMemo(() => matchArray(abiParameter.type), [ abiParameter.type ]);
  if (arrayMatch) {
    if (!Array.isArray(data)) {
      if (arrayMatch.itemType === 'tuple' && mode === 'preview' && 'components' in abiParameter) {
        const previewData = Object.fromEntries(abiParameter.components.map(({ name }) => ([ name ?? '', undefined ])));
        return <ItemArray abiParameter={ abiParameter } data={ [ previewData ] } mode={ mode } level={ level } arrayMatch={ arrayMatch }/>;
      }
      return <ItemPrimitive abiParameter={ abiParameter } data={ data || (mode === 'preview' ? undefined : '[ ]') } level={ level }/>;
    }
    return <ItemArray abiParameter={ abiParameter } data={ data } level={ level } mode={ mode } arrayMatch={ arrayMatch }/>;
  }

  const tupleMatch = abiParameter.type.includes('tuple');
  if (tupleMatch) {
    return <ItemTuple abiParameter={ abiParameter } data={ data } mode={ mode } level={ level }/>;
  }

  return <ItemPrimitive abiParameter={ abiParameter } data={ data } level={ level }/>;
};

export default React.memo(Item);
