import { Box, HStack, Flex } from '@chakra-ui/react';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  apiKey: string;
  name: string;
  isLoading?: boolean;
}

const ApiKeySnippet = ({ apiKey, name, isLoading }: Props) => {
  return (
    <HStack gap={ 2 } alignItems="start">
      <IconSvg name="key" boxSize={ 6 } color="icon.primary" isLoading={ isLoading }/>
      <Box>
        <Flex alignItems={{ base: 'flex-start', lg: 'center' }}>
          <Skeleton loading={ isLoading } display="inline-block" fontWeight={ 600 } mr={ 1 }>
            <span>{ apiKey }</span>
          </Skeleton>
          <CopyToClipboard text={ apiKey } isLoading={ isLoading }/>
        </Flex>
        { name && (
          <Skeleton loading={ isLoading } display="inline-block" fontSize="sm" color="text.secondary" mt={ 1 }>
            <span>{ name }</span>
          </Skeleton>
        ) }
      </Box>
    </HStack>
  );
};

export default React.memo(ApiKeySnippet);
