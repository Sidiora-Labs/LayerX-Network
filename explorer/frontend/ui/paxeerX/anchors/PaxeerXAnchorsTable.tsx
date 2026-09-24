import React from 'react';

import type { PaxeerXAnchorsItem } from 'types/api/paxeerXLists';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import PaxeerXAnchorsTableItem from './PaxeerXAnchorsTableItem';

interface Props {
  items: Array<PaxeerXAnchorsItem>;
  top: number;
  isLoading?: boolean;
}

const PaxeerXAnchorsTable = ({ items, top, isLoading }: Props) => {
  return (
    <TableRoot minW="900px">
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader width="160px">Checkpoint height</TableColumnHeader>
          <TableColumnHeader width="160px">Sealed height</TableColumnHeader>
          <TableColumnHeader width="30%">State root</TableColumnHeader>
          <TableColumnHeader width="20%">Block</TableColumnHeader>
          <TableColumnHeader width="20%">
            Age
            <TimeFormatToggle/>
          </TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items.map((item, index) => (
          <PaxeerXAnchorsTableItem
            key={ item.checkpoint_id + (isLoading ? index : '') }
            item={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default PaxeerXAnchorsTable;
