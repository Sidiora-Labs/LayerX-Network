import { Box } from '@chakra-ui/react';
import React from 'react';

import type { VerifiedContract } from 'types/api/contracts';

import VerifiedContractsListItem from './VerifiedContractsListItem';

const VerifiedContractsList = ({ data, isLoading }: { data: Array<VerifiedContract>; isLoading: boolean }) => {
  return (
    <Box>
      { data.map((item, index) => (
        <VerifiedContractsListItem
          key={ item.address.hash + (isLoading ? index : '') }
          data={ item }
          isLoading={ isLoading }
        />
      )) }
    </Box>
  );
};

export default React.memo(VerifiedContractsList);
