import { Text } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXActivityItem } from 'types/api/paxeerX';

import { TableBody, TableColumnHeader, TableHeader, TableRoot, TableRow } from 'toolkit/chakra/table';

import ActivityListItem from './ActivityListItem';

export interface Props {
  items: Array<PaxeerXActivityItem>;
  isLoading?: boolean;
}

// One feed for chain-side and kernel-side activity, each row carrying its rung on the status ladder.
const ActivityList = ({ items, isLoading }: Props) => {
  if (items.length === 0) {
    return <Text color="text.secondary">There is no Paxeer X activity for this account yet.</Text>;
  }

  return (
    <TableRoot data-label="paxeer-x-activity">
      <TableHeader>
        <TableRow>
          <TableColumnHeader width="20%">Action</TableColumnHeader>
          <TableColumnHeader width="30%">Transaction</TableColumnHeader>
          <TableColumnHeader width="20%">Block</TableColumnHeader>
          <TableColumnHeader width="20%" isNumeric>Amount</TableColumnHeader>
          <TableColumnHeader width="10%">Status</TableColumnHeader>
        </TableRow>
      </TableHeader>
      <TableBody>
        { items.map((item) => (
          <ActivityListItem key={ `${ item.hash }-${ item.kind }-${ item.block_number }` } item={ item } isLoading={ isLoading }/>
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(ActivityList);
