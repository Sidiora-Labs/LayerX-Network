import React from 'react';

import type { AddressEpochRewardsItem } from 'types/api/address';

import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import EpochEntity from 'ui/shared/entities/epoch/EpochEntity';
import EpochRewardTypeTag from 'ui/shared/EpochRewardTypeTag';
import ListItemMobileGrid from 'ui/shared/ListItemMobile/ListItemMobileGrid';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import TokenValue from 'ui/shared/value/TokenValue';

type Props = {
  item: AddressEpochRewardsItem;
  isLoading?: boolean;
};

const AddressEpochRewardsListItem = ({ item, isLoading }: Props) => {
  return (
    <ListItemMobileGrid.Container gridTemplateColumns="100px auto">

      <ListItemMobileGrid.Label isLoading={ isLoading }>Epoch #</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <EpochEntity number={ String(item.epoch_number) } noIcon isLoading={ isLoading }/>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Age</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <TimeWithTooltip
          timestamp={ item.block_timestamp }
          isLoading={ isLoading }
          color="text.secondary"
          display="inline-block"
        />
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Reward type</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <EpochRewardTypeTag type={ item.type } isLoading={ isLoading }/>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Associated address</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <AddressEntity
          address={ item.associated_account }
          isLoading={ isLoading }
        />
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Value</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <TokenValue
          amount={ item.amount }
          token={ item.token }
          loading={ isLoading }
        />
      </ListItemMobileGrid.Value>

    </ListItemMobileGrid.Container>
  );
};

export default AddressEpochRewardsListItem;
