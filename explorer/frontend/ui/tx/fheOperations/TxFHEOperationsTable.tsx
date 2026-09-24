import { Box } from '@chakra-ui/react';
import React from 'react';

import type { FheOperation } from 'types/api/fheOperations';

import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import { TableBody, TableColumnHeader, TableHeader, TableRoot, TableRow } from 'toolkit/chakra/table';
import TxFHEOperationsTableItem from 'ui/tx/fheOperations/TxFHEOperationsTableItem';

interface Props {
  data: Array<FheOperation>;
  isLoading?: boolean;
}

const TxFHEOperationsTable = ({ data, isLoading }: Props) => {
  return (
    <AddressHighlightProvider>
      <Box maxW="100%" overflowX="auto" hideBelow="lg">
        <TableRoot tableLayout="fixed" minWidth="900px" w="100%">
          <TableHeader>
            <TableRow>
              <TableColumnHeader width="10%">Index</TableColumnHeader>
              <TableColumnHeader width="15%">Operation</TableColumnHeader>
              <TableColumnHeader width="12%">Type</TableColumnHeader>
              <TableColumnHeader width="12%">FHE type</TableColumnHeader>
              <TableColumnHeader width="12%">Mode</TableColumnHeader>
              <TableColumnHeader width="12%">HCU cost</TableColumnHeader>
              <TableColumnHeader width="12%">HCU depth</TableColumnHeader>
              <TableColumnHeader width="24%">Caller</TableColumnHeader>
            </TableRow>
          </TableHeader>
          <TableBody>
            { data.map((op) => (
              <TxFHEOperationsTableItem
                key={ op.log_index }
                { ...op }
                isLoading={ isLoading }
              />
            )) }
          </TableBody>
        </TableRoot>
      </Box>
    </AddressHighlightProvider>
  );
};

export default React.memo(TxFHEOperationsTable);
