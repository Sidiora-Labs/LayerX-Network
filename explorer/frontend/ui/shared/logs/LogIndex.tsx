import type { HTMLChakraProps } from '@chakra-ui/react';
import { Center } from '@chakra-ui/react';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';

interface Props extends HTMLChakraProps<'div'> {
  isLoading?: boolean;
}

const LogIndex = ({ children, isLoading, ...props }: Props) => {
  return (
    <Tooltip content="Log index">
      <Skeleton loading={ isLoading } asChild>
        <Center
          color={ isLoading ? 'transparent' : { _light: 'blue.600', _dark: 'gray.50' } }
          bgColor={ isLoading ? undefined : { _light: 'blue.50', _dark: 'gray.600' } }
          borderRadius="base"
          px={ 2 }
          { ...props }
        >
          { children }
        </Center>
      </Skeleton>
    </Tooltip>
  );
};

export default React.memo(LogIndex);
