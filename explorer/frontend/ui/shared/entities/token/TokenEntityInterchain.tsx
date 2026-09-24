import type { JsxStyleProps } from '@chakra-ui/react';
import React from 'react';

import type { ExternalChain } from 'types/externalChains';

import config from 'configs/app';

import type { EntityProps } from './TokenEntity';
import TokenEntity from './TokenEntity';
import TokenEntityExternal from './TokenEntityExternal';

interface Props extends EntityProps, JsxStyleProps {
  chain: ExternalChain | undefined;
}

const TokenEntityInterchain = ({ chain, ...props }: Props) => {

  const isCurrentChain = chain?.id === config.chain.id;

  if (isCurrentChain) {
    return <TokenEntity { ...props }/>;
  }

  return <TokenEntityExternal { ...props } chain={ chain }/>;
};

export default React.memo(TokenEntityInterchain);
