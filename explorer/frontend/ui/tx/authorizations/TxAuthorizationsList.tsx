import { Box } from '@chakra-ui/react';
import React from 'react';

import type { TxAuthorization } from 'types/api/transaction';

import TxAuthorizationsListItem from './TxAuthorizationsListItem';

interface Props {
  data: Array<TxAuthorization> | undefined;
  isLoading?: boolean;
}

const TxAuthorizationsList = ({ data, isLoading }: Props) => {
  return (
    <Box>
      { data?.map((item, index) => <TxAuthorizationsListItem key={ item.nonce.toString() + (isLoading ? index : '') } { ...item } isLoading={ isLoading }/>) }
    </Box>
  );
};

export default TxAuthorizationsList;
