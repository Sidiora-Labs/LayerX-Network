import { chakra } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs/routes';

import * as TxEntity from './TxEntity';

const TxEntityZetaChainCC = (props: TxEntity.EntityProps) => {
  const defaultHref = route({ pathname: '/cc/tx/[hash]', query: { hash: props.hash } });

  return <TxEntity.default { ...props } icon={{ name: 'interop' }} href={ props.href ?? defaultHref }/>;
};

export default chakra(TxEntityZetaChainCC);
