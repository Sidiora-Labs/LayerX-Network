import React from 'react';

import type { InteropMessage } from 'types/api/interop';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import InteropMessagesTableItem from './InteropMessagesTableItem';

interface Props {
  items?: Array<InteropMessage>;
  top: number;
  isLoading?: boolean;
}

const InteropMessagesTable = ({ items, top, isLoading }: Props) => {
  return (
    <TableRoot tableLayout="auto">
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader/>
          <TableColumnHeader>Message</TableColumnHeader>
          <TableColumnHeader>
            Timestamp
            <TimeFormatToggle/>
          </TableColumnHeader>
          <TableColumnHeader>Status</TableColumnHeader>
          <TableColumnHeader>Source tx</TableColumnHeader>
          <TableColumnHeader>Destination tx</TableColumnHeader>
          <TableColumnHeader>Sender</TableColumnHeader>
          <TableColumnHeader>In/Out</TableColumnHeader>
          <TableColumnHeader>Target</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { items?.map((item, index) => (
          <InteropMessagesTableItem
            key={ item.init_transaction_hash + '_' + index }
            item={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(InteropMessagesTable);
