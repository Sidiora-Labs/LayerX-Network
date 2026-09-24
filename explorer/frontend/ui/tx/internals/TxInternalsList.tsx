import { Box } from '@chakra-ui/react';
import React from 'react';

import type { InternalTransaction } from 'types/api/internalTransaction';

import TxInternalsListItem from 'ui/tx/internals/TxInternalsListItem';

const TxInternalsList = ({ data, isLoading }: { data: Array<InternalTransaction>; isLoading?: boolean }) => {
  return (
    <Box>
      { data.map((item, index) => <TxInternalsListItem key={ item.index.toString() + (isLoading ? index : '') } { ...item } isLoading={ isLoading }/>) }
    </Box>
  );
};

export default TxInternalsList;
