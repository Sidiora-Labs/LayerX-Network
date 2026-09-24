import type { HTMLChakraProps } from '@chakra-ui/react';
import { Box, Flex } from '@chakra-ui/react';
import { clamp } from 'es-toolkit';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';

interface Props extends Omit<HTMLChakraProps<'div'>, 'direction'> {
  value: number;
  colorScheme?: 'green' | 'gray';
  isLoading?: boolean;
}

const WIDTH = 50;

const Utilization = ({ value, colorScheme = 'green', isLoading, ...rest }: Props, ref: React.Ref<HTMLDivElement>) => {
  const valueString = (clamp(value * 100 || 0, 0, 100)).toLocaleString(undefined, { maximumFractionDigits: 2 }) + '%';
  const color = colorScheme === 'gray' ? 'text.secondary' : 'green.500';

  return (
    <Flex alignItems="center" columnGap={ 2 } { ...rest } ref={ ref }>
      <Skeleton loading={ isLoading } w={ `${ WIDTH }px` } h="4px" borderRadius="full" overflow="hidden">
        <Box bg={{ _light: 'blackAlpha.200', _dark: 'whiteAlpha.200' }} h="100%">
          <Box bg={ color } w={ valueString } h="100%"/>
        </Box>
      </Skeleton>
      <Skeleton loading={ isLoading } color={ color } fontWeight="bold">
        <span>
          { valueString }
        </span>
      </Skeleton>
    </Flex>
  );
};

export default React.memo(React.forwardRef(Utilization));
