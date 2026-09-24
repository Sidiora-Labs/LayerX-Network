import type { BoxProps } from '@chakra-ui/react';
import { Box, Text } from '@chakra-ui/react';
import React from 'react';

interface Props extends BoxProps {
  text?: string;
}

export const ContentLoader = React.memo(({ text, ...props }: Props) => {
  return (
    <Box display="inline-block" { ...props }>
      <Box
        width="100%"
        height="6px"
        position="relative"
        _after={{
          content: `" "`,
          position: 'absolute',
          width: '60px',
          height: '6px',
          animation: `fromLeftToRight 700ms ease-in-out infinite alternate`,
          left: '0%',
          top: 0,
          backgroundColor: 'blue.300',
          borderRadius: 'full',
        }}
      />
      <Text mt={ 6 } color="text.secondary">
        { text || 'Loading data, please wait...' }
      </Text>
    </Box>
  );
});
