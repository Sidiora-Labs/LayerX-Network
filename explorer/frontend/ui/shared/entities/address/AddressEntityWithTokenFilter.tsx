import { chakra } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs-routes';

import config from 'configs/app';

import * as AddressEntity from './AddressEntity';

interface Props extends AddressEntity.EntityProps {
  tokenHash: string;
  tokenSymbol: string;
}

const AddressEntityWithTokenFilter = (props: Props) => {

  if (!config.features.advancedFilter.isEnabled) {
    return <AddressEntity.default { ...props }/>;
  }

  const defaultHref = route({
    pathname: '/advanced-filter',
    query: {
      ...props.query,
      to_address_hashes_to_include: [ props.address.hash ],
      from_address_hashes_to_include: [ props.address.hash ],
      token_contract_address_hashes_to_include: [ props.tokenHash ],
      token_contract_symbols_to_include: [ props.tokenSymbol ],
    },
  });

  return (
    <AddressEntity.default { ...props } href={ props.href ?? defaultHref }/>
  );
};

export default chakra(AddressEntityWithTokenFilter);
