import { Box } from '@chakra-ui/react';
import React from 'react';

import type { TxStateChange } from 'types/api/txStateChanges';

import TxStateListItem from 'ui/tx/state/TxStateListItem';

interface Props {
  data: Array<TxStateChange>;
  isLoading?: boolean;
}

const TxStateList = ({ data, isLoading }: Props) => {
  return (
    <Box>
      { data.map((item, index) => <TxStateListItem key={ index } data={ item } isLoading={ isLoading }/>) }
    </Box>
  );
};

export default TxStateList;
