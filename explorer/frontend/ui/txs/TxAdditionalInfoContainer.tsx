import { Box, Separator } from '@chakra-ui/react';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import { Skeleton } from 'toolkit/chakra/skeleton';
import DataFetchAlert from 'ui/shared/DataFetchAlert';

import TxAdditionalInfoContent from './TxAdditionalInfoContent';

interface Props {
  hash: string;
}

const TxAdditionalInfoContainer = ({ hash }: Props) => {
  const { data, isError, isPending } = useApiQuery('general:tx', {
    pathParams: { hash },
    queryOptions: {
      refetchOnMount: false,
    },
  });

  if (isPending) {
    return (
      <Box>
        <Skeleton loading w="80px" h="24px" borderRadius="sm" mb={ 3 }/>
        <Box>
          <Skeleton loading w="110px" h="16px" borderRadius="full" mb={ 3 }/>
          <Skeleton loading w="100%" h="16px" borderRadius="full"/>
        </Box>
        <Separator my={ 4 }/>
        <Box>
          <Skeleton loading w="110px" h="16px" borderRadius="full" mb={ 3 }/>
          <Skeleton loading w="100%" h="16px" borderRadius="full"/>
        </Box>
        <Separator my={ 4 }/>
        <Box>
          <Skeleton loading w="110px" h="16px" borderRadius="full" mb={ 3 }/>
          <Skeleton loading w="100%" h="16px" borderRadius="full"/>
        </Box>
        <Separator my={ 4 }/>
        <Box>
          <Skeleton loading w="110px" h="16px" borderRadius="full" mb={ 3 }/>
          <Skeleton loading w="75%" h="16px" borderRadius="full"/>
          <Skeleton loading w="75%" h="16px" borderRadius="full" mt={ 1 }/>
          <Skeleton loading w="75%" h="16px" borderRadius="full" mt={ 1 }/>
        </Box>
        <Separator my={ 4 }/>
        <Skeleton loading w="80px" h="16px" borderRadius="full"/>
      </Box>
    );
  }

  if (isError) {
    return <DataFetchAlert/>;
  }

  return <TxAdditionalInfoContent tx={ data }/>;
};

export default React.memo(TxAdditionalInfoContainer);
