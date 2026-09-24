import React from 'react';

import type { ScrollL2TxnBatch } from 'types/api/scrollL2';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import ScrollL2TxnBatchesTableItem from './ScrollL2TxnBatchesTableItem';

type Props = {
  items: Array<ScrollL2TxnBatch>;
  top: number;
  isLoading?: boolean;
};

const ScrollL2TxnBatchesTable = ({ items, top, isLoading }: Props) => {
  return (
    <TableRoot tableLayout="auto" minW="1000px">
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader>Batch #</TableColumnHeader>
          <TableColumnHeader>Container</TableColumnHeader>
          <TableColumnHeader>Status</TableColumnHeader>
          <TableColumnHeader>Committed block</TableColumnHeader>
          <TableColumnHeader>Committed txn hash</TableColumnHeader>
          <TableColumnHeader>
            Timestamp
            <TimeFormatToggle/>
          </TableColumnHeader>
          <TableColumnHeader>Finalized block</TableColumnHeader>
          <TableColumnHeader>Finalized txn hash</TableColumnHeader>
          <TableColumnHeader isNumeric>Blocks</TableColumnHeader>
          <TableColumnHeader isNumeric>Txn</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items.map((item, index) => (
          <ScrollL2TxnBatchesTableItem
            key={ item.number + (isLoading ? String(index) : '') }
            item={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default ScrollL2TxnBatchesTable;
