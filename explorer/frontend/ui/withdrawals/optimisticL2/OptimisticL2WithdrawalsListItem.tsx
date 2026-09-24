import React from 'react';

import type { OptimisticL2WithdrawalsItem } from 'types/api/optimisticL2';

import config from 'configs/app';
import dayjs from 'lib/date/dayjs';
import { layerLabels } from 'lib/rollups/utils';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import ListItemMobileGrid from 'ui/shared/ListItemMobile/ListItemMobileGrid';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

import OptimisticL2WithdrawalsItemStatus from './OptimisticL2WithdrawalsItemStatus';

const rollupFeature = config.features.rollup;

type Props = { item: OptimisticL2WithdrawalsItem; isLoading?: boolean };

const OptimisticL2WithdrawalsListItem = ({ item, isLoading }: Props) => {
  const timeToEnd = item.challenge_period_end ? dayjs(item.challenge_period_end).fromNow(true) + ' left' : null;

  if (!rollupFeature.isEnabled || rollupFeature.type !== 'optimistic') {
    return null;
  }

  return (
    <ListItemMobileGrid.Container>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Msg nonce</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <Skeleton loading={ isLoading } display="inline-block">
          { item.msg_nonce_version + '-' + item.msg_nonce }
        </Skeleton>
      </ListItemMobileGrid.Value>

      { item.from && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>From</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <AddressEntity
              address={ item.from }
              isLoading={ isLoading }
              truncation="constant"
            />
          </ListItemMobileGrid.Value>
        </>
      ) }

      <ListItemMobileGrid.Label isLoading={ isLoading }>{ layerLabels.current } txn hash</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <TxEntity
          isLoading={ isLoading }
          hash={ item.l2_transaction_hash }
          truncation="constant_long"
        />
      </ListItemMobileGrid.Value>

      { item.l2_timestamp && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Age</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <TimeWithTooltip
              timestamp={ item.l2_timestamp }
              isLoading={ isLoading }
              display="inline-block"
            />
          </ListItemMobileGrid.Value>
        </>
      ) }

      <ListItemMobileGrid.Label isLoading={ isLoading }>Status</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <OptimisticL2WithdrawalsItemStatus data={ item } isLoading={ isLoading }/>
      </ListItemMobileGrid.Value>

      { item.l1_transaction_hash && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>{ layerLabels.parent } txn hash</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <TxEntityL1
              isLoading={ isLoading }
              hash={ item.l1_transaction_hash }
              truncation="constant_long"
              noCopy
            />
          </ListItemMobileGrid.Value>
        </>
      ) }

      { timeToEnd && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Time left</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>{ timeToEnd }</ListItemMobileGrid.Value>
        </>
      ) }

    </ListItemMobileGrid.Container>
  );
};

export default OptimisticL2WithdrawalsListItem;
