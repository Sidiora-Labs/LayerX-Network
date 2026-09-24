import React from 'react';

import type { WatchlistAddress } from 'types/api/account';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';

import WatchlistTableItem from './WatchListTableItem';

interface Props {
  data?: Array<WatchlistAddress>;
  isLoading?: boolean;
  onEditClick: (data: WatchlistAddress) => void;
  onDeleteClick: (data: WatchlistAddress) => void;
  top: number;
  hasEmail: boolean;
}

const WatchlistTable = ({ data, isLoading, onDeleteClick, onEditClick, top, hasEmail }: Props) => {
  return (
    <TableRoot minWidth="600px">
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader width="70%">Address</TableColumnHeader>
          <TableColumnHeader width="30%">Private tag</TableColumnHeader>
          <TableColumnHeader width="160px">Email notification</TableColumnHeader>
          <TableColumnHeader width="108px"></TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { data?.map((item, index) => (
          <WatchlistTableItem
            key={ item.address_hash + (isLoading ? index : '') }
            item={ item }
            isLoading={ isLoading }
            onDeleteClick={ onDeleteClick }
            onEditClick={ onEditClick }
            hasEmail={ hasEmail }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default WatchlistTable;
