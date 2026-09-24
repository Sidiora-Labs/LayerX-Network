import type { JsxStyleProps } from '@chakra-ui/react';
import React from 'react';

import type { ExternalChain } from 'types/externalChains';

import { stripTrailingSlash } from 'toolkit/utils/url';

import type { EntityProps } from './TxEntity';
import TxEntity from './TxEntity';

interface Props extends EntityProps, JsxStyleProps {
  chain: ExternalChain | undefined;
}

const TxEntityExternal = ({ chain, ...props }: Props) => {

  const defaultHref = (() => {
    if (!chain || !chain.explorer_url) {
      return;
    }

    try {
      const url = new URL(
        stripTrailingSlash(chain.explorer_url) +
            (chain.route_templates?.tx || '/tx/{hash}').replace('{hash}', props.hash),
      );
      return url.toString();
    } catch (error) {}
  })();

  const href = props.href ?? defaultHref;

  return (
    <TxEntity
      { ...props }
      href={ href }
      noLink={ props.noLink || !href }
      link={{ external: true }}
      chain={ chain }
    />
  );
};

export default React.memo(TxEntityExternal);
