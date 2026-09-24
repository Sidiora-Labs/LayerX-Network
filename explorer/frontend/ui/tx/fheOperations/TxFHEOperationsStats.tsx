import { Box } from '@chakra-ui/react';
import React from 'react';

import StatsWidget from 'ui/shared/stats/StatsWidget';

interface Props {
  totalHcu: number;
  maxDepthHcu: number;
  operationCount: number;
  isLoading?: boolean;
}

const TxFHEOperationsStats = ({ totalHcu, maxDepthHcu, operationCount, isLoading }: Props) => {
  return (
    <Box
      display="grid"
      gridTemplateColumns={{ base: '1fr', lg: 'repeat(3, calc(33.333% - 8px))' }}
      gap={ 3 }
      mb={ 6 }
    >
      <StatsWidget
        label="Total HCU"
        hint="Sum of all Homomorphic Computation Units consumed by FHE operations in this transaction"
        value={ (totalHcu || 0).toLocaleString() }
        isLoading={ isLoading }
      />
      <StatsWidget
        label="Max Depth HCU"
        hint="Maximum HCU consumed at any single depth level in the FHE operation tree"
        value={ (maxDepthHcu || 0).toLocaleString() }
        isLoading={ isLoading }
      />
      <StatsWidget
        label="Operations"
        hint="Total number of FHE operations executed in this transaction"
        value={ operationCount.toLocaleString() }
        isLoading={ isLoading }
      />
    </Box>
  );
};

export default React.memo(TxFHEOperationsStats);
