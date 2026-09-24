import { chakra, Grid } from '@chakra-ui/react';
import React from 'react';

interface Props {
  children: React.ReactNode;
  className?: string;
  isLoading: boolean;
}

const OptimisticL2TxnBatchBlobWrapper = ({ children, className, isLoading }: Props) => {
  return (
    <Grid
      className={ className }
      columnGap={ 3 }
      rowGap="10px"
      p={ 4 }
      bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.100' }}
      gridTemplateColumns="auto 1fr"
      borderRadius="base"
      w="100%"
      h={ isLoading ? '140px' : undefined }
      textStyle="sm"
    >
      { isLoading ? null : children }
    </Grid>
  );
};

export default React.memo(chakra(OptimisticL2TxnBatchBlobWrapper));
