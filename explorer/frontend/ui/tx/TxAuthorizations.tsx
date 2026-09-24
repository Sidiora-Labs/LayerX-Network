import { Box } from '@chakra-ui/react';
import React from 'react';

import DataListDisplay from 'ui/shared/DataListDisplay';
import TxPendingAlert from 'ui/tx/TxPendingAlert';
import TxSocketAlert from 'ui/tx/TxSocketAlert';

import TxAuthorizationsList from './authorizations/TxAuthorizationsList';
import TxAuthorizationsTable from './authorizations/TxAuthorizationsTable';
import type { TxQuery } from './useTxQuery';

interface Props {
  txQuery: TxQuery;
}

const TxAuthorizations = ({ txQuery }: Props) => {

  if (!txQuery.isPlaceholderData && !txQuery.isError && !txQuery.data?.status) {
    return txQuery.socketStatus ? <TxSocketAlert status={ txQuery.socketStatus }/> : <TxPendingAlert/>;
  }

  const content = (
    <>
      <Box hideFrom="lg">
        <TxAuthorizationsList data={ txQuery.data?.authorization_list } isLoading={ txQuery.isPlaceholderData }/>
      </Box>
      <Box hideBelow="lg">
        <TxAuthorizationsTable data={ txQuery.data?.authorization_list } isLoading={ txQuery.isPlaceholderData }/>
      </Box>
    </>
  );

  return (
    <DataListDisplay
      isError={ txQuery.isError }
      itemsNum={ txQuery.data?.authorization_list?.length }
      emptyText="There are no authorizations for this transaction."
    >
      { content }
    </DataListDisplay>
  );
};

export default TxAuthorizations;
