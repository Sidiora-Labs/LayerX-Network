import React from 'react';

import type { ArbitrumL2TxnBatchesItem } from 'types/api/arbitrumL2';

import { layerLabels } from 'lib/rollups/utils';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import ArbitrumL2TxnBatchesTableItem from './ArbitrumL2TxnBatchesTableItem';

type Props = {
  items: Array<ArbitrumL2TxnBatchesItem>;
  top: number;
  isLoading?: boolean;
};

const ArbitrumL2TxnBatchesTable = ({ items, top, isLoading }: Props) => {
  return (
    <TableRoot tableLayout="auto" minW="1000px">
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader>Batch #</TableColumnHeader>
          <TableColumnHeader>{ layerLabels.parent } status</TableColumnHeader>
          <TableColumnHeader>{ layerLabels.parent } block</TableColumnHeader>
          <TableColumnHeader>Block count</TableColumnHeader>
          <TableColumnHeader>{ layerLabels.parent } transaction</TableColumnHeader>
          <TableColumnHeader>
            Timestamp
            <TimeFormatToggle/>
          </TableColumnHeader>
          <TableColumnHeader>Txn count</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items.map((item, index) => (
          <ArbitrumL2TxnBatchesTableItem
            key={ item.number + (isLoading ? String(index) : '') }
            item={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default ArbitrumL2TxnBatchesTable;
