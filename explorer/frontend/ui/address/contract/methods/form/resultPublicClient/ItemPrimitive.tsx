import { chakra } from '@chakra-ui/react';
import React from 'react';
import type { AbiParameter } from 'viem';

import { route } from 'nextjs-routes';

import { Link } from 'toolkit/chakra/link';
import CopyToClipboard from 'ui/shared/CopyToClipboard';

import ItemLabel from './ItemLabel';
import { printRowOffset } from './utils';

function castValueToString(value: unknown): string {
  switch (typeof value) {
    case 'string':
      return value === '' ? `""` : value;
    case 'undefined':
      return '';
    case 'number':
      return value.toLocaleString(undefined, { useGrouping: false });
    case 'bigint':
      return value.toString();
    case 'boolean':
    default:
      return String(value);
  }
}

interface Props {
  abiParameter: AbiParameter;
  data: unknown;
  level: number;
  hideLabel?: boolean;
}

const ItemPrimitive = ({ abiParameter, data, level, hideLabel }: Props) => {

  const value = (() => {
    if (abiParameter.type === 'address' && typeof data === 'string') {
      return (
        <>
          <Link href={ route({ pathname: '/address/[hash]', query: { hash: data } }) }>{ data }</Link>
          <CopyToClipboard text={ data } boxSize={ 4 } verticalAlign="sub"/>
        </>
      );
    }

    return <span>{ castValueToString(data) }</span>;
  })();

  return (
    <chakra.span display="block">
      <span>{ printRowOffset(level) }</span>
      { !hideLabel && <ItemLabel abiParameter={ abiParameter }/> }
      { value }
    </chakra.span>
  );
};

export default React.memo(ItemPrimitive);
