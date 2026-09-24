import { Spinner } from '@chakra-ui/react';
import React from 'react';

import { Alert } from 'toolkit/chakra/alert';

const TxPendingAlert = () => {
  return (
    <Alert startElement={ <Spinner size="sm" my={ 1 }/> }>
      This transaction is pending confirmation.
    </Alert>
  );
};

export default TxPendingAlert;
