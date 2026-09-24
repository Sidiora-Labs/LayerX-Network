import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { ExternalChain } from 'types/externalChains';

import { route } from 'nextjs/routes';

import useZetaChainConfig from 'ui/zetaChain/useZetaChainConfig';

import * as TxEntity from './TxEntity';

type Props = {
  chainId: string;
} & Omit<TxEntity.EntityProps, 'chain'>;

const TxEntityZetaChainExternal = (props: Props) => {
  const { data: chainsConfig } = useZetaChainConfig();
  const chain = chainsConfig?.find((chain) => chain.id.toString() === props.chainId);

  const defaultHref = (() => {
    if (chain && 'tx_url_template' in chain && chain.tx_url_template) {
      return chain.tx_url_template.replace('{hash}', props.hash);
    }
    return route({ pathname: '/tx/[hash]', query: { hash: props.hash } }, { chain: chain as ExternalChain, external: true });
  })();

  return <TxEntity.default { ...props } href={ props.href ?? defaultHref } link={{ external: true }}/>;
};

export default chakra(TxEntityZetaChainExternal);
