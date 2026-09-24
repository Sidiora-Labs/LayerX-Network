import type { IconProps } from '@chakra-ui/react';
import { Icon } from '@chakra-ui/react';
import React from 'react';

import LogoIcon from 'icons/networks/logo-placeholder.svg';

export const ChartWatermark = React.memo((props: IconProps) => {
  return (
    <Icon
      position="absolute"
      opacity={ 0.1 }
      top="50%"
      left="50%"
      transform="translate(-50%, -50%)"
      pointerEvents="none"
      viewBox="0 0 114 20"
      color={{ _light: 'link.primary', _dark: 'white' }}
      { ...props }
    >
      <LogoIcon/>
    </Icon>
  );
});
