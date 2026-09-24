import React from 'react';

import type { DepositsItem } from 'types/api/deposits';

import { TableCell, TableRow } from 'toolkit/chakra/table';
import BeaconChainDepositSignature from 'ui/shared/beacon/BeaconChainDepositSignature';
import BeaconChainDepositStatusTag from 'ui/shared/beacon/BeaconChainDepositStatusTag';
import BeaconChainValidatorLink from 'ui/shared/beacon/BeaconChainValidatorLink';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

type Props = {
  item: DepositsItem;
  view: 'list' | 'address' | 'block';
  isLoading?: boolean;
};

const BeaconChainDepositsTableItem = ({ item, view, isLoading }: Props) => {
  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <TxEntity
          hash={ item.transaction_hash }
          isLoading={ isLoading }
          truncation="constant_long"
          noIcon
        />
      </TableCell>
      { view !== 'block' && (
        <TableCell verticalAlign="middle">
          <BlockEntity
            number={ item.block_number }
            hash={ item.block_hash }
            isLoading={ isLoading }
            textStyle="sm"
            noIcon
          />
        </TableCell>
      ) }
      { view !== 'block' && (
        <TableCell verticalAlign="middle">
          <TimeWithTooltip
            timestamp={ item.block_timestamp }
            isLoading={ isLoading }
            color="text.secondary"
            display="inline-block"
          />
        </TableCell>
      ) }
      <TableCell verticalAlign="middle">
        <NativeCoinValue amount={ item.amount } loading={ isLoading } noSymbol/>
      </TableCell>
      { view !== 'address' && (
        <TableCell verticalAlign="middle">
          <AddressEntity
            address={ item.from_address }
            isLoading={ isLoading }
            truncation="constant"
          />
        </TableCell>
      ) }
      <TableCell verticalAlign="middle" maxW="200px" overflow="hidden">
        <BeaconChainValidatorLink pubkey={ item.pubkey } isLoading={ isLoading }/>
      </TableCell>
      <TableCell verticalAlign="middle" maxW="200px" overflow="hidden">
        <BeaconChainDepositSignature signature={ item.signature } isLoading={ Boolean(isLoading) }/>
      </TableCell>
      <TableCell verticalAlign="middle">
        <BeaconChainDepositStatusTag status={ item.status } isLoading={ Boolean(isLoading) }/>
      </TableCell>
    </TableRow>
  );
};

export default BeaconChainDepositsTableItem;
