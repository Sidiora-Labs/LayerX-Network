import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { ArbitrumL2MessagesItem } from 'types/api/arbitrumL2';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import BlockEntityL1 from 'ui/shared/entities/block/BlockEntityL1';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';
import ArbitrumL2MessageStatus from 'ui/shared/statusTag/ArbitrumL2MessageStatus';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

import type { MessagesDirection } from './ArbitrumL2Messages';

const rollupFeature = config.features.rollup;

type Props = { item: ArbitrumL2MessagesItem; isLoading?: boolean; direction: MessagesDirection };

const ArbitrumL2MessagesTableItem = ({ item, direction, isLoading }: Props) => {
  if (!rollupFeature.isEnabled || rollupFeature.type !== 'arbitrum') {
    return null;
  }

  const l1TxHash = direction === 'from-rollup' ? item.completion_transaction_hash : item.origination_transaction_hash;
  const l2TxHash = direction === 'from-rollup' ? item.origination_transaction_hash : item.completion_transaction_hash;

  return (
    <TableRow>
      { direction === 'to-rollup' && (
        <TableCell verticalAlign="middle">
          { item.origination_transaction_block_number ? (
            <BlockEntityL1
              number={ item.origination_transaction_block_number }
              isLoading={ isLoading }
              fontWeight={ 600 }
              noIcon
            />
          ) : <chakra.span color="text.secondary">N/A</chakra.span> }
        </TableCell>
      ) }
      { direction === 'from-rollup' && (
        <TableCell verticalAlign="middle">
          <AddressEntity
            address={{ hash: item.origination_address_hash }}
            truncation="constant"
            isLoading={ isLoading }
            fontWeight={ 600 }
          />
        </TableCell>
      ) }
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading }>
          <span>{ item.id }</span>
        </Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        { l2TxHash ? (
          <TxEntity
            isLoading={ isLoading }
            hash={ l2TxHash }
            truncation="constant_long"
            noIcon
          />
        ) : (
          <chakra.span color="text.secondary">
            N/A
          </chakra.span>
        ) }
      </TableCell>
      <TableCell verticalAlign="middle" pr={ 12 }>
        <TimeWithTooltip
          timestamp={ item.origination_timestamp }
          isLoading={ isLoading }
          color="text.secondary"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        { item.status === 'confirmed' && direction === 'from-rollup' ?
          <Link href={ route({ pathname: '/txn-withdrawals', query: { q: item.origination_transaction_hash } }) }>Ready for relay</Link> :
          <ArbitrumL2MessageStatus status={ item.status } isLoading={ isLoading }/> }
      </TableCell>
      <TableCell verticalAlign="middle">
        { l1TxHash ? (
          <TxEntityL1
            isLoading={ isLoading }
            hash={ l1TxHash }
            truncation="constant_long"
            noIcon
            noCopy
          />
        ) : (
          <chakra.span color="text.secondary">
            N/A
          </chakra.span>
        ) }
      </TableCell>
    </TableRow>
  );
};

export default ArbitrumL2MessagesTableItem;
