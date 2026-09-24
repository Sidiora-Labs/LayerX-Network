import { Box } from '@chakra-ui/react';
import React from 'react';

import type { ValidatorStability } from 'types/api/validators';

import ValidatorsListItem from './ValidatorsListItem';

const ValidatorsList = ({ data, isLoading }: { data: Array<ValidatorStability>; isLoading: boolean }) => {
  return (
    <Box>
      { data.map((item, index) => (
        <ValidatorsListItem
          key={ item.address.hash + (isLoading ? index : '') }
          data={ item }
          isLoading={ isLoading }
        />
      )) }
    </Box>
  );
};

export default React.memo(ValidatorsList);
