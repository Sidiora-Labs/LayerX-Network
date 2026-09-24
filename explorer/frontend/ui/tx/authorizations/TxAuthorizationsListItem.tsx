import { HStack } from '@chakra-ui/react';
import React from 'react';

import type { TxAuthorization } from 'types/api/transaction';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import TxAuthorizationStatus from 'ui/shared/statusTag/TxAuthorizationStatus';

interface Props extends TxAuthorization {
  isLoading?: boolean;
}

const TxAuthorizationsListItem = ({ address_hash: addressHash, authority, chain_id: chainId, nonce, isLoading, status }: Props) => {
  return (
    <ListItemMobile rowGap={ 3 } fontSize="sm">
      <HStack gap={ 3 } w="100%">
        <Skeleton loading={ isLoading } fontWeight={ 500 }>Authority</Skeleton>
        <AddressEntity address={{ hash: authority }} isLoading={ isLoading } noIcon/>
      </HStack>
      <HStack gap={ 3 } w="100%">
        <Skeleton loading={ isLoading } fontWeight={ 500 } flexShrink={ 0 }>Delegated address</Skeleton>
        <AddressEntity address={{ hash: addressHash }} isLoading={ isLoading } noIcon/>
      </HStack>
      <HStack gap={ 3 }>
        <Skeleton loading={ isLoading } fontWeight={ 500 }>Chain</Skeleton>
        <Skeleton loading={ isLoading } color="text.secondary">{ chainId === Number(config.chain.id) ? 'this' : 'any' }</Skeleton>
      </HStack>
      <HStack gap={ 3 }>
        <Skeleton loading={ isLoading } fontWeight={ 500 }>Nonce</Skeleton>
        <Skeleton loading={ isLoading } color="text.secondary">{ nonce }</Skeleton>
      </HStack>
      <HStack gap={ 3 }>
        <Skeleton loading={ isLoading } fontWeight={ 500 }>Status</Skeleton>
        <TxAuthorizationStatus status={ status } loading={ isLoading }/>
      </HStack>
    </ListItemMobile>
  );
};

export default TxAuthorizationsListItem;
