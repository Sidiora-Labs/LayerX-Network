import { Box, useToken } from '@chakra-ui/react';
import React from 'react';

interface Props {
  isVerified: boolean;
}

const AddressIconDelegated = ({ isVerified }: Props) => {
  const bgColor = useToken('colors', 'bg.primary')[0];

  return (
    <Box position="absolute" boxSize="14px" top="-2px" right="-2px" color={ isVerified ? 'green.500' : 'icon.primary' }>
      <svg
        viewBox="0 0 14 14"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
      >
        <rect width="14" height="6" rx="2" x="0" y="4" fill={ bgColor }/>
        <rect width="6" height="14" rx="2" x="4" y="0" fill={ bgColor }/>
        <rect width="10" height="2" rx="0.5" x="2" y="6" fill="currentColor"/>
        <rect width="2" height="10" rx="0.5" x="6" y="2" fill="currentColor"/>
      </svg>
    </Box>
  );
};

export default React.memo(AddressIconDelegated);
