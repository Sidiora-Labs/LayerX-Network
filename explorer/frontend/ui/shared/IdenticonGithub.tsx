import { Box, chakra } from '@chakra-ui/react';
import dynamic from 'next/dynamic';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';

const Identicon = dynamic<{ bg: string; string: string; size: number }>(
  async() => {
    const lib = await import('react-identicons');
    return typeof lib === 'object' && 'default' in lib ? lib.default : lib;
  },
  {
    loading: () => <Skeleton loading w="100%" h="100%"/>,
    ssr: false,
  },
);

interface Props {
  className?: string;
  iconSize: number;
  seed: string;
}

const IdenticonGithub = ({ iconSize, seed }: Props) => {
  return (
    <Box
      boxSize={ `${ iconSize * 2 }px` }
      transformOrigin="left top"
      transform="scale(0.5)"
      borderRadius="full"
      overflow="hidden"
      bg={{ _light: 'gray.100', _dark: 'white' }}
    >
      <Identicon
        bg="transparent"
        string={ seed }
        // the displayed size is doubled for retina displays and then scaled down
        size={ iconSize * 2 }
      />
    </Box>
  );
};

export default React.memo(chakra(IdenticonGithub));
