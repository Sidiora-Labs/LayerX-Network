import React from 'react';

import type { ZkSyncBatchesItem } from 'types/api/zkSyncL2';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import ZkSyncTxnBatchesTableItem from './ZkSyncTxnBatchesTableItem';

type Props = {
  items: Array<ZkSyncBatchesItem>;
  top: number;
  isLoading?: boolean;
};

const ZkSyncTxnBatchesTable = ({ items, top, isLoading }: Props) => {
  return (
    <TableRoot minW="1000px">
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader width="40%">Batch #</TableColumnHeader>
          <TableColumnHeader width="60%">Status</TableColumnHeader>
          <TableColumnHeader width="180px">
            Timestamp
            <TimeFormatToggle/>
          </TableColumnHeader>
          <TableColumnHeader width="120px">Txn count</TableColumnHeader>
          <TableColumnHeader width="210px">Commit tx</TableColumnHeader>
          <TableColumnHeader width="210px">Prove tx</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items.map((item, index) => (
          <ZkSyncTxnBatchesTableItem
            key={ item.number + (isLoading ? String(index) : '') }
            item={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default ZkSyncTxnBatchesTable;
