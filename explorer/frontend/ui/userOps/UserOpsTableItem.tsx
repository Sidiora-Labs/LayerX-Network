import React from 'react';

import type { UserOpsItem } from 'types/api/userOps';
import type { ClusterChainConfig } from 'types/multichain';

import config from 'configs/app';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressStringOrParam from 'ui/shared/entities/address/AddressStringOrParam';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import UserOpEntity from 'ui/shared/entities/userOp/UserOpEntity';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import UserOpStatus from 'ui/shared/userOps/UserOpStatus';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

type Props = {
  item: UserOpsItem;
  isLoading?: boolean;
  showTx: boolean;
  showSender: boolean;
  chainData?: ClusterChainConfig;
};

const UserOpsTableItem = ({ item, isLoading, showTx, showSender, chainData }: Props) => {
  return (
    <TableRow>
      { chainData && (
        <TableCell verticalAlign="middle">
          <ChainIcon data={ chainData } isLoading={ isLoading }/>
        </TableCell>
      ) }
      <TableCell verticalAlign="middle">
        <UserOpEntity hash={ item.hash } isLoading={ isLoading } noIcon fontWeight={ 700 } truncation="constant_long" noCopy/>
      </TableCell>
      <TableCell verticalAlign="middle">
        <TimeWithTooltip
          timestamp={ item.timestamp }
          isLoading={ isLoading }
          color="text.secondary"
          display="inline-block"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <UserOpStatus status={ item.status } isLoading={ isLoading }/>
      </TableCell>
      { showSender && (
        <TableCell verticalAlign="middle">
          <AddressStringOrParam
            address={ item.address }
            isLoading={ isLoading }
            truncation="constant"
          />
        </TableCell>
      ) }
      { showTx && (
        <TableCell verticalAlign="middle">
          <TxEntity
            hash={ item.transaction_hash }
            isLoading={ isLoading }
            truncation="constant"
            noIcon
          />
        </TableCell>
      ) }
      <TableCell verticalAlign="middle">
        <BlockEntity
          number={ Number(item.block_number) }
          isLoading={ isLoading }
          textStyle="sm"
          noIcon
        />
      </TableCell>
      { !config.UI.views.tx.hiddenFields?.tx_fee && (
        <TableCell verticalAlign="middle" isNumeric>
          <NativeCoinValue amount={ item.fee } loading={ isLoading } noSymbol/>
        </TableCell>
      ) }
    </TableRow>
  );
};

export default UserOpsTableItem;
