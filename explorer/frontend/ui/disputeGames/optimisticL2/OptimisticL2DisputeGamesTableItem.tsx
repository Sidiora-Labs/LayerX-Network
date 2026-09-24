import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { OptimisticL2DisputeGamesItem } from 'types/api/optimisticL2';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import BlockEntityL2 from 'ui/shared/entities/block/BlockEntityL2';
import HashStringShorten from 'ui/shared/HashStringShorten';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

const faultProofSystemFeature = config.features.faultProofSystem;

type Props = { item: OptimisticL2DisputeGamesItem; isLoading?: boolean };

const OptimisticL2DisputeGamesTableItem = ({ item, isLoading }: Props) => {
  if (!faultProofSystemFeature.isEnabled) {
    return null;
  }

  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block">{ item.index }</Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block">{ item.game_type }</Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <Flex overflow="hidden" w="100%" alignItems="center">
          <Skeleton loading={ isLoading }>
            <HashStringShorten hash={ item.contract_address_hash } type="long"/>
          </Skeleton>
          <CopyToClipboard text={ item.contract_address_hash } ml={ 2 } isLoading={ isLoading }/>
        </Flex>
      </TableCell>
      <TableCell verticalAlign="middle">
        <BlockEntityL2
          isLoading={ isLoading }
          number={ item.l2_block_number }
          noIcon
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <TimeWithTooltip
          timestamp={ item.created_at }
          isLoading={ isLoading }
          display="inline-block"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block">{ item.status }</Skeleton>
      </TableCell>
      <TableCell>
        <TimeWithTooltip
          timestamp={ item.resolved_at }
          fallbackText="N/A"
          isLoading={ isLoading }
          display="inline-block"
        />
      </TableCell>
    </TableRow>
  );
};

export default OptimisticL2DisputeGamesTableItem;
