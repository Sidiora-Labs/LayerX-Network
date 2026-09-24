import React from 'react';

import type { TxStateChange } from 'types/api/txStateChanges';

import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TxStateTableItem from 'ui/tx/state/TxStateTableItem';

interface Props {
  data: Array<TxStateChange>;
  isLoading?: boolean;
  top: number;
}

const TxStateTable = ({ data, isLoading, top }: Props) => {
  return (
    <AddressHighlightProvider>
      <TableRoot minWidth="1000px" w="100%">
        <TableHeaderSticky top={ top }>
          <TableRow>
            <TableColumnHeader width="140px">Type</TableColumnHeader>
            <TableColumnHeader width="160px">Address</TableColumnHeader>
            <TableColumnHeader width="33%" isNumeric>Before</TableColumnHeader>
            <TableColumnHeader width="33%" isNumeric>After</TableColumnHeader>
            <TableColumnHeader width="33%" isNumeric>Change</TableColumnHeader>
            <TableColumnHeader width="150px" minW="80px" maxW="150px">Token ID</TableColumnHeader>
          </TableRow>
        </TableHeaderSticky>
        <TableBody>
          { data.map((item, index) => <TxStateTableItem data={ item } key={ index } isLoading={ isLoading }/>) }
        </TableBody>
      </TableRoot>
    </AddressHighlightProvider>
  );
};

export default React.memo(TxStateTable);
