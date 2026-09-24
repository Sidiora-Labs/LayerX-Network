import { Spinner, chakra } from '@chakra-ui/react';
import React from 'react';

import { Alert } from 'toolkit/chakra/alert';

interface Props {
  isLoading?: boolean;
  className?: string;
}

const ServiceDegradationWarning = ({ isLoading, className }: Props) => {
  return (
    <Alert
      loading={ isLoading }
      status="info"
      className={ className }
      startElement={ <Spinner size="sm" my="3px" flexShrink={ 0 }/> }
    >
      Data sync in progress... page will refresh automatically once data is available
    </Alert>
  );
};

export default React.memo(chakra(ServiceDegradationWarning));
