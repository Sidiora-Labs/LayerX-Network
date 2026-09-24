import type { BoxProps } from '@chakra-ui/react';
import React from 'react';

import IconSvg from 'ui/shared/IconSvg';

const TokenLogoPlaceholder = (props: BoxProps) => {
  return (
    <IconSvg
      fontWeight={ 600 }
      bgColor={{ _light: 'gray.200', _dark: 'gray.600' }}
      color={{ _light: 'gray.400', _dark: 'gray.200' }}
      borderRadius="base"
      name="token-placeholder"
      transitionProperty="background-color,color"
      transitionDuration="normal"
      transitionTimingFunction="ease"
      { ...props }
    />
  );
};

export default TokenLogoPlaceholder;
