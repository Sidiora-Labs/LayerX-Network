import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXActivityItem } from 'types/api/paxeerX';

import { route } from 'nextjs/routes';

import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import StatusLadderBadge from 'ui/shared/statusLadder/StatusLadderBadge';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

import { activityKindLabel, assetLabel, formatAmount } from './utils';

export interface Props {
  item: PaxeerXActivityItem;
  isLoading?: boolean;
}

const SIDE_LABELS: Record<PaxeerXActivityItem['side'], string> = {
  chain: 'Chain',
  kernel: 'Kernel',
};

const ActivityListItem = ({ item, isLoading }: Props) => {
  const amount = item.amount === null ?
    null :
    `${ formatAmount(item.amount, item.asset) } ${ assetLabel(item.asset) }`;

  return (
    <TableRow data-activity={ item.hash }>
      <TableCell verticalAlign="middle">
        <Flex flexDirection="column" rowGap={ 1 }>
          <Skeleton loading={ isLoading } fontWeight={ 600 }>{ activityKindLabel(item.kind) }</Skeleton>
          <Skeleton loading={ isLoading } color="text.secondary" textStyle="sm">{ SIDE_LABELS[item.side] }</Skeleton>
        </Flex>
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } overflow="hidden" textOverflow="ellipsis">
          <Link href={ route({ pathname: '/tx/[hash]', query: { hash: item.hash } }) }>{ item.hash }</Link>
        </Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading }>
          <Link href={ route({ pathname: '/block/[height_or_hash]', query: { height_or_hash: String(item.block_number) } }) }>
            { item.block_number }
          </Link>
        </Skeleton>
        <TimeWithTooltip timestamp={ item.timestamp } isLoading={ isLoading } color="text.secondary" textStyle="sm" display="block"/>
      </TableCell>
      <TableCell verticalAlign="middle" isNumeric>
        <Skeleton loading={ isLoading } display="inline-block">
          { amount === null ? <Text as="span" color="text.secondary">—</Text> : amount }
        </Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <StatusLadderBadge rung={ item.status } loading={ isLoading }/>
      </TableCell>
    </TableRow>
  );
};

export default React.memo(ActivityListItem);
