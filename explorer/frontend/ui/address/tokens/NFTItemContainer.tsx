import { Box, chakra } from '@chakra-ui/react';
import React from 'react';

type Props = {
  children: React.ReactNode;
  className?: string;
};

const NFTItemContainer = ({ children, className }: Props) => {
  return (
    <Box
      w={{ base: '100%', lg: '210px' }}
      border="1px solid"
      borderColor={{ _light: 'blackAlpha.100', _dark: 'whiteAlpha.200' }}
      borderRadius="12px"
      p="10px"
      fontSize="sm"
      fontWeight={ 500 }
      lineHeight="20px"
      className={ className }
    >
      { children }
    </Box>
  );
};

export default chakra(NFTItemContainer);
