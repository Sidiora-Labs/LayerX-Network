import type { BoxProps } from '@chakra-ui/react';
import { Box, HStack, VStack } from '@chakra-ui/react';
import React from 'react';

import { TableBody, TableCell, TableRoot, TableRow } from 'toolkit/chakra/table';
import FallbackBox from 'ui/shared/fallbacks/FallbackBox';
import IconSvg from 'ui/shared/IconSvg';

const LatestTxsFallback = (props: BoxProps) => {
  return (
    <Box { ...props }>
      <Box color="text.secondary" textStyle="sm">Failed to load data. Please try again later.</Box>
      <TableRoot mt={ 3 } borderTopWidth="1px" borderColor="border.divider">
        <TableBody>
          { Array.from({ length: 2 }).map((_, index) => (
            <TableRow key={ index }>
              <TableCell pl={ 0 } pr={{ base: 3, lg: 4 }} w={{ base: '220px', lg: '33%' }}>
                <VStack alignItems="stretch">
                  <HStack>
                    <IconSvg name="transactions" boxSize={ 5 } color={{ _light: 'gray.300', _dark: 'whiteAlpha.300' }}/>
                    <FallbackBox w="90px" bgColor={{ _light: 'blue.50', _dark: 'blue.800' }}/>
                    <FallbackBox w="90px" bgColor={{ _light: 'purple.50', _dark: 'purple.800' }}/>
                  </HStack>
                  <FallbackBox w="100%"/>
                </VStack>
              </TableCell>
              <TableCell px={{ base: 3, lg: 4 }}>
                <VStack alignItems="stretch">
                  <FallbackBox w="100%" my={ 1 }/>
                  <FallbackBox w="100%"/>
                </VStack>
              </TableCell>
              <TableCell pl={{ base: 3, lg: 4 }} pr={{ base: 0, lg: 2 }}>
                <VStack alignItems="stretch">
                  <FallbackBox w="100%" my={ 1 }/>
                  <FallbackBox w="100%"/>
                </VStack>
              </TableCell>
            </TableRow>
          )) }
        </TableBody>
      </TableRoot>
    </Box>
  );
};

export default React.memo(LatestTxsFallback);
