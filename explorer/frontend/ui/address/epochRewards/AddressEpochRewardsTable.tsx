import React from 'react';

import type { AddressEpochRewardsItem } from 'types/api/address';

import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import AddressEpochRewardsTableItem from './AddressEpochRewardsTableItem';

type Props = {
  items: Array<AddressEpochRewardsItem>;
  isLoading?: boolean;
  top: number;
};

const AddressEpochRewardsTable = ({ items, isLoading, top }: Props) => {
  return (
    <AddressHighlightProvider>
      <TableRoot minW="1000px" style={{ tableLayout: 'auto' }}>
        <TableHeaderSticky top={ top }>
          <TableRow>
            <TableColumnHeader>
              Epoch
              <TimeFormatToggle/>
            </TableColumnHeader>
            <TableColumnHeader>Reward type</TableColumnHeader>
            <TableColumnHeader>Associated address</TableColumnHeader>
            <TableColumnHeader isNumeric>Value</TableColumnHeader>
          </TableRow>
        </TableHeaderSticky>
        <TableBody>
          { items.map((item, index) => {
            return (
              <AddressEpochRewardsTableItem
                key={ item.epoch_number + item.type + item.account.hash + item.associated_account.hash + (isLoading ? String(index) : '') }
                item={ item }
                isLoading={ isLoading }
              />
            );
          }) }
        </TableBody>
      </TableRoot>
    </AddressHighlightProvider>
  );
};

export default AddressEpochRewardsTable;
