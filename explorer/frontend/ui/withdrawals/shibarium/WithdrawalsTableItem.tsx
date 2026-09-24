import React from 'react';

import type { ShibariumWithdrawalsItem } from 'types/api/shibarium';

import config from 'configs/app';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressStringOrParam from 'ui/shared/entities/address/AddressStringOrParam';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

const feature = config.features.rollup;

type Props = { item: ShibariumWithdrawalsItem; isLoading?: boolean };

const WithdrawalsTableItem = ({ item, isLoading }: Props) => {
  if (!(feature.isEnabled && feature.type === 'shibarium')) {
    return null;
  }

  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <BlockEntity
          number={ item.l2_block_number }
          isLoading={ isLoading }
          textStyle="sm"
          fontWeight={ 600 }
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <TxEntity
          isLoading={ isLoading }
          hash={ item.l2_transaction_hash }
          textStyle="sm"
          truncation="constant_long"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <TxEntityL1
          isLoading={ isLoading }
          hash={ item.l1_transaction_hash }
          truncation="constant_long"
          textStyle="sm"
          noCopy
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <AddressStringOrParam
          address={ item.user }
          isLoading={ isLoading }
          truncation="constant"
          noCopy
        />
      </TableCell>
      <TableCell verticalAlign="middle" pr={ 12 }>
        <TimeWithTooltip
          timestamp={ item.timestamp }
          isLoading={ isLoading }
          display="inline-block"
          color="text.secondary"
        />
      </TableCell>
    </TableRow>
  );
};

export default WithdrawalsTableItem;
