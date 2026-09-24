import { chakra } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs-routes';

import config from 'configs/app';

import * as TokenEntity from './TokenEntity';

interface Props extends TokenEntity.EntityProps {
  addressHash: string;
}

const TokenEntityWithAddressFilter = (props: Props) => {

  if (!config.features.advancedFilter.isEnabled) {
    return <TokenEntity.default { ...props }/>;
  }

  const defaultHref = route({
    pathname: '/advanced-filter',
    query: {
      ...props.query,
      to_address_hashes_to_include: [ props.addressHash ],
      from_address_hashes_to_include: [ props.addressHash ],
      token_contract_address_hashes_to_include: [ props.token.address_hash ],
      token_contract_symbols_to_include: [ props.token.symbol ?? '' ],
    },
  });

  return (
    <TokenEntity.default { ...props } href={ props.href ?? defaultHref }/>
  );
};

export default chakra(TokenEntityWithAddressFilter);
