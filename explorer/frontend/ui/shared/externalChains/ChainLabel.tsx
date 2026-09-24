import type { StackProps } from '@chakra-ui/react';
import { Box, Flex, HStack } from '@chakra-ui/react';
import React from 'react';

import type { ExternalChain } from 'types/externalChains';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import CopyToClipboard from 'ui/shared/CopyToClipboard';

import ChainIcon from './ChainIcon';

interface Props extends StackProps {
  data: Omit<ExternalChain, 'explorer_url'> | undefined;
  isLoading?: boolean;
  fallback?: React.ReactNode;
}

const ChainLabel = ({ data, isLoading, fallback, ...rest }: Props) => {
  if (!data) {
    return fallback || null;
  }

  const content = (
    <>
      <Box fontWeight={ 600 }>{ data.name }</Box>
      <Flex alignItems="center" justifyContent="center">
        ChainID: { data.id }
        <CopyToClipboard text={ data.id } noTooltip/>
      </Flex>
    </>
  );

  return (
    <HStack w="full" whiteSpace="nowrap" { ...rest }>
      <ChainIcon data={ data } isLoading={ isLoading } noTooltip/>
      <Tooltip content={ content } interactive>
        <Skeleton loading={ isLoading } overflow="hidden" textOverflow="ellipsis">
          { data.name }
        </Skeleton>
      </Tooltip>
    </HStack>
  );
};

export default React.memo(ChainLabel);
