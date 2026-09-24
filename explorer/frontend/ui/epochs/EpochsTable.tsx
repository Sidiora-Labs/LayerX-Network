import React from 'react';

import type { CeloEpochListItem } from 'types/api/epochs';

import config from 'configs/app';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import EpochsTableItem from './EpochsTableItem';

interface Props {
  items: Array<CeloEpochListItem>;
  isLoading?: boolean;
  top: number;
};

const EpochsTable = ({ items, isLoading, top }: Props) => {
  return (
    <TableRoot minW="1100px">
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader w="280px">
            Epoch
            <TimeFormatToggle/>
          </TableColumnHeader>
          <TableColumnHeader w="120px">Status</TableColumnHeader>
          <TableColumnHeader w="25%">Block range</TableColumnHeader>
          <TableColumnHeader w="25%" isNumeric>Community { config.chain.currency.symbol }</TableColumnHeader>
          <TableColumnHeader w="25%" isNumeric>Carbon offset { config.chain.currency.symbol }</TableColumnHeader>
          <TableColumnHeader w="25%" isNumeric>Total { config.chain.currency.symbol }</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items.map((item, index) => {
          return (
            <EpochsTableItem
              key={ item.number + (isLoading ? String(index) : '') }
              item={ item }
              isLoading={ isLoading }
            />
          );
        }) }
      </TableBody>
    </TableRoot>
  );
};

export default EpochsTable;
