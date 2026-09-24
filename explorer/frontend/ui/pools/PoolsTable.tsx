import React from 'react';

import type { Pool } from 'types/api/pools';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';

import PoolsTableItem from './PoolsTableItem';

type Props = {
  items: Array<Pool>;
  page: number;
  isLoading?: boolean;
  top?: number;
};

const PoolsTable = ({ items, page, isLoading, top }: Props) => {
  return (
    <TableRoot minWidth="900px">
      <TableHeaderSticky top={ top ?? ACTION_BAR_HEIGHT_DESKTOP }>
        <TableRow>
          <TableColumnHeader width="70%">Pool</TableColumnHeader>
          <TableColumnHeader width="30%">DEX </TableColumnHeader>
          <TableColumnHeader width="130px" isNumeric>Liquidity</TableColumnHeader>
          <TableColumnHeader width="75px" isNumeric>View in</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items.map((item, index) => (
          <PoolsTableItem key={ item.pool_id + (isLoading ? index : '') } item={ item } index={ index } page={ page } isLoading={ isLoading }/>
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default PoolsTable;
