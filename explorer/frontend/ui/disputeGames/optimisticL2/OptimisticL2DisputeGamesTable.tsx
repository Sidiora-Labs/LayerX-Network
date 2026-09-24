import React from 'react';

import type { OptimisticL2DisputeGamesItem } from 'types/api/optimisticL2';

import { layerLabels } from 'lib/rollups/utils';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import OptimisticL2DisputeGamesTableItem from './OptimisticL2DisputeGamesTableItem';

type Props = {
  items: Array<OptimisticL2DisputeGamesItem>;
  top: number;
  isLoading?: boolean;
};

const OptimisticL2DisputeGamesTable = ({ items, top, isLoading }: Props) => {
  return (
    <TableRoot tableLayout="auto" minW="950px">
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader>Index</TableColumnHeader>
          <TableColumnHeader>Game type</TableColumnHeader>
          <TableColumnHeader>Address</TableColumnHeader>
          <TableColumnHeader>{ layerLabels.current } block #</TableColumnHeader>
          <TableColumnHeader>
            Timestamp
            <TimeFormatToggle/>
          </TableColumnHeader>
          <TableColumnHeader>Status</TableColumnHeader>
          <TableColumnHeader>
            Resolved
            <TimeFormatToggle/>
          </TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items.map((item, index) => (
          <OptimisticL2DisputeGamesTableItem
            key={ String(item.index) + (isLoading ? index : '') }
            item={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default OptimisticL2DisputeGamesTable;
