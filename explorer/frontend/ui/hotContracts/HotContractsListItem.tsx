import { HStack } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import type { HotContract } from 'types/api/contracts';

import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import { Reputation } from 'ui/shared/entities/token/TokenEntity';
import EntityTags from 'ui/shared/EntityTags/EntityTags';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

interface Props {
  data: HotContract;
  isLoading?: boolean;
  exchangeRate: string | null;
}

const HotContractsListItem = ({ data, isLoading, exchangeRate }: Props) => {
  const protocolTags = data?.contract_address?.metadata?.tags?.filter(tag => tag.tagType === 'protocol');

  return (
    <ListItemMobile rowGap={ 3 } py={ 4 } textStyle="sm">
      <HStack justifyContent="space-between" width="100%">
        <AddressEntity
          address={ data.contract_address }
          isLoading={ isLoading }
        />
        <Reputation value={ data.contract_address.reputation ?? null }/>
      </HStack>
      { protocolTags && protocolTags.length > 0 && (
        <EntityTags
          isLoading={ isLoading }
          tags={ protocolTags }
          noColors
        />
      ) }
      <HStack>
        <Skeleton loading={ isLoading } fontWeight={ 500 } w="100px">Txn count</Skeleton>
        <Skeleton loading={ isLoading }>
          <span>{ Number(data.transactions_count).toLocaleString() }</span>
        </Skeleton>
      </HStack>
      <HStack>
        <Skeleton loading={ isLoading } fontWeight={ 500 } w="100px">Gas used</Skeleton>
        <Skeleton loading={ isLoading }>
          <span>{ BigNumber(data.total_gas_used || 0).toFormat() }</span>
        </Skeleton>
      </HStack>
      <HStack alignItems="flex-start">
        <Skeleton loading={ isLoading } fontWeight={ 500 } w="100px">Balance</Skeleton>
        <NativeCoinValue
          amount={ data.balance }
          loading={ isLoading }
          exchangeRate={ exchangeRate }
          flexWrap="wrap"
        />
      </HStack>
    </ListItemMobile>
  );
};

export default React.memo(HotContractsListItem);
