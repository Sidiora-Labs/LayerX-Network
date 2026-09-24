import { chakra } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import type { ZkEvmL2DepositsItem } from 'types/api/zkEvmL2';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import BlockEntityL1 from 'ui/shared/entities/block/BlockEntityL1';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import SimpleValue from 'ui/shared/value/SimpleValue';

const rollupFeature = config.features.rollup;

type Props = { item: ZkEvmL2DepositsItem; isLoading?: boolean };

const ZkEvmL2DepositsTableItem = ({ item, isLoading }: Props) => {
  if (!rollupFeature.isEnabled || rollupFeature.type !== 'zkEvm') {
    return null;
  }

  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <BlockEntityL1
          number={ item.block_number }
          isLoading={ isLoading }
          textStyle="sm"
          fontWeight={ 600 }
          noIcon
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading }>
          <span>{ item.index }</span>
        </Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <TxEntityL1
          isLoading={ isLoading }
          hash={ item.l1_transaction_hash }
          truncation="constant_long"
          noIcon
          textStyle="sm"
          noCopy
        />
      </TableCell>
      <TableCell verticalAlign="middle" pr={ 12 }>
        <TimeWithTooltip
          timestamp={ item.timestamp }
          isLoading={ isLoading }
          color="text.secondary"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        { item.l2_transaction_hash ? (
          <TxEntity
            isLoading={ isLoading }
            hash={ item.l2_transaction_hash }
            textStyle="sm"
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
        <SimpleValue
          value={ BigNumber(item.value) }
          loading={ isLoading }
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block">
          <span>{ item.symbol }</span>
        </Skeleton>
      </TableCell>
    </TableRow>
  );
};

export default ZkEvmL2DepositsTableItem;
