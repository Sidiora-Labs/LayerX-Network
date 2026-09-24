import BigNumber from 'bignumber.js';
import React from 'react';

import type { OptimisticL2DepositsItem } from 'types/api/optimisticL2';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressEntityL1 from 'ui/shared/entities/address/AddressEntityL1';
import BlockEntityL1 from 'ui/shared/entities/block/BlockEntityL1';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

const rollupFeature = config.features.rollup;

type Props = { item: OptimisticL2DepositsItem; isLoading?: boolean };

const OptimisticDepositsTableItem = ({ item, isLoading }: Props) => {

  if (!rollupFeature.isEnabled || rollupFeature.type !== 'optimistic') {
    return null;
  }

  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <BlockEntityL1
          number={ item.l1_block_number }
          isLoading={ isLoading }
          fontWeight={ 600 }
          noIcon
        />
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
          timestamp={ item.l1_block_timestamp }
          isLoading={ isLoading }
          color="text.secondary"
          display="inline-block"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <TxEntityL1
          isLoading={ isLoading }
          hash={ item.l1_transaction_hash }
          truncation="constant_long"
          noIcon
          noCopy
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <AddressEntityL1
          address={{ hash: item.l1_transaction_origin, name: '', is_contract: false, is_verified: false, ens_domain_name: null, implementations: null }}
          isLoading={ isLoading }
          truncation="constant"
          noCopy
        />
      </TableCell>
      <TableCell verticalAlign="middle" isNumeric>
        <Skeleton loading={ isLoading } color="text.secondary" display="inline-block">
          <span>{ BigNumber(item.l2_transaction_gas_limit).toFormat() }</span>
        </Skeleton>
      </TableCell>
    </TableRow>
  );
};

export default OptimisticDepositsTableItem;
