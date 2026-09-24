import React from 'react';

import type * as tac from '@blockscout/tac-operation-lifecycle-types';

import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import TacOperationsTableItem from './TacOperationsTableItem';

type Props = {
  items: Array<tac.OperationBriefDetails>;
  isLoading?: boolean;
};

const TacOperationsTable = ({ items, isLoading }: Props) => {
  return (
    <AddressHighlightProvider>
      <TableRoot minW="950px">
        <TableHeaderSticky top={ 68 }>
          <TableRow>
            <TableColumnHeader w="200px">Status</TableColumnHeader>
            <TableColumnHeader w="100%">Operation</TableColumnHeader>
            <TableColumnHeader w="200px">
              Timestamp
              <TimeFormatToggle/>
            </TableColumnHeader>
            <TableColumnHeader w="250px">Sender</TableColumnHeader>
          </TableRow>
        </TableHeaderSticky>
        <TableBody>
          { items.map((item, index) => (
            <TacOperationsTableItem key={ String(item.operation_id) + (isLoading ? index : '') } item={ item } isLoading={ isLoading }/>
          )) }
        </TableBody>
      </TableRoot>
    </AddressHighlightProvider>
  );
};

export default TacOperationsTable;
