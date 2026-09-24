import React from 'react';

import type { OptimisticL2WithdrawalsItem } from 'types/api/optimisticL2';

import config from 'configs/app';
import dayjs from 'lib/date/dayjs';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

import OptimisticL2WithdrawalsItemStatus from './OptimisticL2WithdrawalsItemStatus';

const rollupFeature = config.features.rollup;

type Props = { item: OptimisticL2WithdrawalsItem; isLoading?: boolean };

const OptimisticL2WithdrawalsTableItem = ({ item, isLoading }: Props) => {
  const timeToEnd = item.challenge_period_end ? dayjs(item.challenge_period_end).fromNow(true) + ' left' : '';

  if (!rollupFeature.isEnabled || rollupFeature.type !== 'optimistic') {
    return null;
  }

  return (
    <TableRow>
      <TableCell verticalAlign="middle" fontWeight={ 600 }>
        <Skeleton loading={ isLoading } display="inline-block">{ item.msg_nonce_version + '-' + item.msg_nonce }</Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        { item.from ? (
          <AddressEntity
            address={ item.from }
            isLoading={ isLoading }
            truncation="constant"
          />
        ) : 'N/A' }
      </TableCell>
      <TableCell verticalAlign="middle">
        <TxEntity
          isLoading={ isLoading }
          hash={ item.l2_transaction_hash }
          truncation="constant_long"
          noIcon
        />
      </TableCell>
      <TableCell verticalAlign="middle" pr={ 12 }>
        <TimeWithTooltip
          timestamp={ item.l2_timestamp }
          fallbackText="N/A"
          isLoading={ isLoading }
          display="inline-block"
          color="text.secondary"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <OptimisticL2WithdrawalsItemStatus data={ item } isLoading={ isLoading }/>
      </TableCell>
      <TableCell verticalAlign="middle">
        { item.l1_transaction_hash ? (
          <TxEntityL1
            isLoading={ isLoading }
            hash={ item.l1_transaction_hash }
            truncation="constant_long"
            noIcon
            noCopy
          />
        ) :
          'N/A'
        }
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } color="text.secondary" minW="50px" minH="20px" display="inline-block">{ timeToEnd }</Skeleton>
      </TableCell>
    </TableRow>
  );
};

export default OptimisticL2WithdrawalsTableItem;
