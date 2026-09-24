import { HStack } from '@chakra-ui/react';
import React from 'react';

import type { MudWorldItem } from 'types/api/mudWorlds';

import { currencyUnits } from 'lib/units';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

type Props = {
  item: MudWorldItem;
  isLoading?: boolean;
};

const MudWorldsListItem = ({
  item,
  isLoading,
}: Props) => {

  return (
    <ListItemMobile rowGap={ 3 }>
      <AddressEntity
        address={ item.address }
        isLoading={ isLoading }
        fontWeight={ 700 }
        mr={ 2 }
        truncation="constant_long"
      />
      <HStack gap={ 3 } maxW="100%" alignItems="flex-start" textStyle="sm">
        <Skeleton loading={ isLoading } fontWeight={ 500 } flexShrink={ 0 }><span>{ `Balance ${ currencyUnits.ether }` }</span></Skeleton>
        <NativeCoinValue
          amount={ item.coin_balance }
          noSymbol
          loading={ isLoading }
          color="text.secondary"
        />
      </HStack>
      <HStack gap={ 3 }>
        <Skeleton loading={ isLoading } fontSize="sm" fontWeight={ 500 }>Txn count</Skeleton>
        <Skeleton loading={ isLoading } fontSize="sm" color="text.secondary">
          <span>{ Number(item.transactions_count).toLocaleString() }</span>
        </Skeleton>
      </HStack>
    </ListItemMobile>
  );
};

export default React.memo(MudWorldsListItem);
