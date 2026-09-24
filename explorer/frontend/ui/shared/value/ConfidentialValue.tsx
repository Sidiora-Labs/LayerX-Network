import type { BoxProps } from '@chakra-ui/react';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';

interface Props extends Omit<BoxProps, 'children'> {
  loading?: boolean;
}

const ConfidentialValue = ({ loading, ...rest }: Props) => {
  return (
    <Skeleton loading={ loading } display="inline-block" { ...rest }>
      •••••
    </Skeleton>
  );
};

export default React.memo(ConfidentialValue);
