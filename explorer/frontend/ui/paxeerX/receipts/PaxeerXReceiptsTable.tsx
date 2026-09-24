import React from 'react';

import type { PaxeerXReceiptsItem } from 'types/api/paxeerXLists';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';

import PaxeerXReceiptsTableItem from './PaxeerXReceiptsTableItem';

interface Props {
  items: Array<PaxeerXReceiptsItem>;
  top: number;
  isLoading?: boolean;
}

const PaxeerXReceiptsTable = ({ items, top, isLoading }: Props) => {
  return (
    <TableRoot minW="900px">
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader width="30%">Receipt ID</TableColumnHeader>
          <TableColumnHeader width="30%">Account</TableColumnHeader>
          <TableColumnHeader width="20%">Status</TableColumnHeader>
          <TableColumnHeader width="20%">Block</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items.map((item, index) => (
          <PaxeerXReceiptsTableItem
            key={ item.id + (isLoading ? index : '') }
            item={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default PaxeerXReceiptsTable;
