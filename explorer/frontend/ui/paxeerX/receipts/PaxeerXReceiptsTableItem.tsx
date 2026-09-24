import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXReceiptsItem } from 'types/api/paxeerXLists';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import HashStringShorten from 'ui/shared/HashStringShorten';
import StatusLadderBadge from 'ui/shared/statusLadder/StatusLadderBadge';

interface Props {
  item: PaxeerXReceiptsItem;
  isLoading?: boolean;
}

const PaxeerXReceiptsTableItem = ({ item, isLoading }: Props) => {
  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <Flex overflow="hidden" w="100%" alignItems="center">
          <Skeleton loading={ isLoading } fontWeight={ 600 }>
            <HashStringShorten hash={ item.id } type="long"/>
          </Skeleton>
          <CopyToClipboard text={ item.id } ml={ 2 } isLoading={ isLoading }/>
        </Flex>
      </TableCell>
      <TableCell verticalAlign="middle">
        { item.account === null ? (
          <Text color="text.secondary">—</Text>
        ) : (
          <Flex overflow="hidden" w="100%" alignItems="center">
            <Skeleton loading={ isLoading }>
              <HashStringShorten hash={ item.account } type="long"/>
            </Skeleton>
            <CopyToClipboard text={ item.account } ml={ 2 } isLoading={ isLoading }/>
          </Flex>
        ) }
      </TableCell>
      <TableCell verticalAlign="middle">
        <StatusLadderBadge rung={ item.status } isLoading={ isLoading }/>
      </TableCell>
      <TableCell verticalAlign="middle">
        <BlockEntity
          isLoading={ isLoading }
          number={ item.block_number }
          fontWeight={ 600 }
          noIcon
        />
      </TableCell>
    </TableRow>
  );
};

export default PaxeerXReceiptsTableItem;
