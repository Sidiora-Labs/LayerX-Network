import { Box } from '@chakra-ui/react';
import React from 'react';

import type { ValidatorsZilliqaItem } from 'types/api/validators';

import ValidatorsListItem from './ValidatorsListItem';

const ValidatorsList = ({ data, isLoading }: { data: Array<ValidatorsZilliqaItem>; isLoading: boolean }) => {
  return (
    <Box>
      { data.map((item, index) => (
        <ValidatorsListItem
          key={ item.bls_public_key + (isLoading ? index : '') }
          data={ item }
          isLoading={ isLoading }
        />
      )) }
    </Box>
  );
};

export default React.memo(ValidatorsList);
