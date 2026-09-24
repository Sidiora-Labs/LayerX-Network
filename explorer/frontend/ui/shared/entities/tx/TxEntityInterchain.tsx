import type { JsxStyleProps } from '@chakra-ui/react';
import React from 'react';

import type { ExternalChain } from 'types/externalChains';

import config from 'configs/app';

import type { EntityProps as TxEntityProps } from './TxEntity';
import TxEntity from './TxEntity';
import TxEntityExternal from './TxEntityExternal';

interface Props extends TxEntityProps, JsxStyleProps {
  chain: ExternalChain | undefined;
}

const TxEntityInterchain = ({ chain, ...props }: Props) => {

  const isCurrentChain = chain?.id === config.chain.id;

  if (isCurrentChain) {
    return <TxEntity { ...props } chain={ chain }/>;
  }

  return <TxEntityExternal { ...props } chain={ chain }/>;
};

export default React.memo(TxEntityInterchain);
