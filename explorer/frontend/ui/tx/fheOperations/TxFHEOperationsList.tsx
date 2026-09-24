import { Box } from '@chakra-ui/react';
import React from 'react';

import type { FheOperation } from 'types/api/fheOperations';

import TxFHEOperationsListItem from 'ui/tx/fheOperations/TxFHEOperationsListItem';

interface Props {
  data: Array<FheOperation>;
  isLoading?: boolean;
}

const TxFHEOperationsList = ({ data, isLoading }: Props) => {
  return (
    <Box hideFrom="lg">
      { data.map((op) => (
        <TxFHEOperationsListItem
          key={ op.log_index }
          { ...op }
          isLoading={ isLoading }
        />
      )) }
    </Box>
  );
};

export default React.memo(TxFHEOperationsList);
