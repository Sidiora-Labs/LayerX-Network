import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { TokenInfo } from 'types/api/token';

import TokenValue from 'ui/shared/value/TokenValue';

interface Props {
  token: TokenInfo;
  value: string;
  decimals: string | null;
}
const FtTokenTransferSnippet = ({ token, value, decimals }: Props) => {
  return (
    <TokenValue
      amount={ value }
      token={ token }
      decimals={ decimals }
      accuracy={ 0 }
      startElement={ <chakra.span color="text.secondary">for </chakra.span> }
    />
  );
};

export default React.memo(FtTokenTransferSnippet);
