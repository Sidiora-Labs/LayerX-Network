import type { BoxProps } from '@chakra-ui/react';
import { Box } from '@chakra-ui/react';
import React from 'react';

const FallbackBox = (props: BoxProps) => {
  return <Box h={ 3 } bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.100' }} borderRadius="sm" { ...props }/>;
};

export default React.memo(FallbackBox);
