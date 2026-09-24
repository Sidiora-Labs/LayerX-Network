import React from 'react';

import { Badge, type BadgeProps } from 'toolkit/chakra/badge';

interface Props extends BadgeProps {
  type: 'in' | 'out';
  isLoading?: boolean;
}

const CrossChainFromToTag = ({ type, isLoading, ...rest }: Props) => {
  return (
    <Badge
      loading={ isLoading }
      colorPalette={ type === 'in' ? 'purple' : 'orange' }
      minW={ 8 }
      justifyContent="center"
      { ...rest }
    >
      { type === 'in' ? 'In' : 'Out' }
    </Badge>
  );
};

export default React.memo(CrossChainFromToTag);
