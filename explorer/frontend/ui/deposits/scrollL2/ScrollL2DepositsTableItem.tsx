import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { ScrollL2MessageItem } from 'types/api/scrollL2';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import BlockEntityL1 from 'ui/shared/entities/block/BlockEntityL1';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

const rollupFeature = config.features.rollup;

type Props = { item: ScrollL2MessageItem; isLoading?: boolean };

const ScrollL2DepositsTableItem = ({ item, isLoading }: Props) => {
  if (!rollupFeature.isEnabled || rollupFeature.type !== 'scroll') {
    return null;
  }

  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <BlockEntityL1
          number={ item.origination_transaction_block_number }
          isLoading={ isLoading }
          fontWeight={ 600 }
          noIcon
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading }>
          <span>{ item.id }</span>
        </Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <TxEntityL1
          isLoading={ isLoading }
          hash={ item.origination_transaction_hash }
          truncation="constant_long"
          noIcon
          noCopy
        />
      </TableCell>
      <TableCell verticalAlign="middle" pr={ 12 }>
        <TimeWithTooltip
          timestamp={ item.origination_timestamp }
          isLoading={ isLoading }
          color="text.secondary"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        { item.completion_transaction_hash ? (
          <TxEntity
            isLoading={ isLoading }
            hash={ item.completion_transaction_hash }
            truncation="constant_long"
            noIcon
          />
        ) : (
          <chakra.span color="text.secondary">
            Pending Claim
          </chakra.span>
        ) }
      </TableCell>
      <TableCell verticalAlign="middle" isNumeric>
        <NativeCoinValue
          amount={ item.value }
          noSymbol
          loading={ isLoading }
        />
      </TableCell>
    </TableRow>
  );
};

export default ScrollL2DepositsTableItem;
