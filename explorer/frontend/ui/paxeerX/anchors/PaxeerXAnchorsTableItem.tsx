import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXAnchorsItem } from 'types/api/paxeerXLists';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import HashStringShorten from 'ui/shared/HashStringShorten';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

interface Props {
  item: PaxeerXAnchorsItem;
  isLoading?: boolean;
}

const PaxeerXAnchorsTableItem = ({ item, isLoading }: Props) => {
  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block" fontWeight={ 600 }>
          { item.checkpoint_height ?? <Text as="span" color="text.secondary">—</Text> }
        </Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block">
          { item.sealed_height ?? <Text as="span" color="text.secondary">—</Text> }
        </Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        { item.state_root === null ? (
          <Text color="text.secondary">—</Text>
        ) : (
          <Flex overflow="hidden" w="100%" alignItems="center">
            <Skeleton loading={ isLoading }>
              <HashStringShorten hash={ item.state_root } type="long"/>
            </Skeleton>
            <CopyToClipboard text={ item.state_root } ml={ 2 } isLoading={ isLoading }/>
          </Flex>
        ) }
      </TableCell>
      <TableCell verticalAlign="middle">
        <BlockEntity
          isLoading={ isLoading }
          number={ item.block_number }
          fontWeight={ 600 }
          noIcon
        />
      </TableCell>
      <TableCell verticalAlign="middle">
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

export default PaxeerXAnchorsTableItem;
