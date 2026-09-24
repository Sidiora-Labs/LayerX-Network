import type { JsxStyleProps } from '@chakra-ui/react';
import React from 'react';

import type { ExternalChain } from 'types/externalChains';

import config from 'configs/app';

import type { EntityProps } from './AddressEntity';
import AddressEntity from './AddressEntity';
import AddressEntityExternal from './AddressEntityExternal';

interface Props extends EntityProps, JsxStyleProps {
  chain: ExternalChain | undefined;
  currentAddress?: string;
}

const AddressEntityInterchain = ({ chain, currentAddress, ...props }: Props) => {

  const isCurrentChain = chain?.id === config.chain.id;
  const isCurrentAddress = isCurrentChain && currentAddress?.toLowerCase() === props.address.hash.toLowerCase();

  if (isCurrentChain) {
    return <AddressEntity { ...props } chain={ chain } noLink={ isCurrentAddress }/>;
  }

  return <AddressEntityExternal { ...props } chain={ chain }/>;
};

export default React.memo(AddressEntityInterchain);
