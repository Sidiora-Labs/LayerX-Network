import type { JsxStyleProps } from '@chakra-ui/react';
import React from 'react';

import type { ExternalChain } from 'types/externalChains';

import { stripTrailingSlash } from 'toolkit/utils/url';

import type { EntityProps } from './TokenEntity';
import TokenEntity from './TokenEntity';

interface Props extends EntityProps, JsxStyleProps {
  chain: ExternalChain | undefined;
}

const TokenEntityExternal = ({ chain, ...props }: Props) => {

  const defaultHref = (() => {
    if (!chain || !chain.explorer_url) {
      return;
    }

    try {
      const url = new URL(
        stripTrailingSlash(chain.explorer_url) +
            (chain.route_templates?.token || '/token/{hash}').replace('{hash}', props.token.address_hash),
      );
      return url.toString();
    } catch (error) {}
  })();

  const href = props.href ?? defaultHref;

  return (
    <TokenEntity
      { ...props }
      href={ href }
      noLink={ props.noLink || !href }
      link={{ external: true }}
    />
  );
};

export default React.memo(TokenEntityExternal);
