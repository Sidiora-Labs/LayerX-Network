import React from 'react';

import type { MudWorldItem } from 'types/api/mudWorlds';

import { currencyUnits } from 'lib/units';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';

import MudWorldsTableItem from './MudWorldsTableItem';

type Props = {
  items: Array<MudWorldItem>;
  top: number;
  isLoading?: boolean;
};

const MudWorldsTable = ({ items, top, isLoading }: Props) => {
  return (
    <TableRoot style={{ tableLayout: 'auto' }}>
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader>Address</TableColumnHeader>
          <TableColumnHeader isNumeric>{ `Balance ${ currencyUnits.ether }` }</TableColumnHeader>
          <TableColumnHeader isNumeric>Txn count</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items.map((item, index) => (
          <MudWorldsTableItem
            key={ String(item.address.hash) + (isLoading ? index : '') }
            item={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default MudWorldsTable;
