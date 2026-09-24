import { chakra } from '@chakra-ui/react';
import React from 'react';

import shortenString from 'lib/shortenString';
import { Tooltip } from 'toolkit/chakra/tooltip';

interface Props {
  hash: string;
  noTooltip?: boolean;
  tooltipInteractive?: boolean;
  type?: 'long' | 'short';
  maxSymbols?: number;
  as?: React.ElementType;
}

const HashStringShorten = ({ hash, noTooltip, as = 'span', type, tooltipInteractive, maxSymbols }: Props) => {
  const charNumber = maxSymbols ?? (type === 'long' ? 16 : 8);
  if (hash.length <= charNumber) {
    return <chakra.span as={ as }>{ hash }</chakra.span>;
  }

  const content = <chakra.span as={ as }>{ shortenString(hash, charNumber) }</chakra.span>;

  if (noTooltip) {
    return content;
  }

  return (
    <Tooltip content={ hash } interactive={ tooltipInteractive }>
      { content }
    </Tooltip>
  );
};

export default HashStringShorten;
