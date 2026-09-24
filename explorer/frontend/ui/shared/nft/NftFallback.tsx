import { chakra } from '@chakra-ui/react';
import React from 'react';

import IconSvg from 'ui/shared/IconSvg';

const NftFallback = ({ className }: { className?: string }) => {
  return (
    <IconSvg
      className={ className }
      name="nft_shield"
      p="50px"
      color={{ _light: 'blackAlpha.500', _dark: 'whiteAlpha.500' }}
      bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
    />
  );
};

export default chakra(NftFallback);
