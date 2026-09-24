import type { BoxProps, HTMLChakraProps } from '@chakra-ui/react';
import { Box, Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';

import CopyToClipboard from './CopyToClipboard';

interface Props {
  data: React.ReactNode;
  title?: string;
  className?: string;
  rightSlot?: React.ReactNode;
  beforeSlot?: React.ReactNode;
  textareaMaxHeight?: BoxProps['maxH'];
  textareaMinHeight?: BoxProps['minH'];
  showCopy?: boolean;
  isLoading?: boolean;
  contentProps?: HTMLChakraProps<'div'>;
}

const RawDataSnippet = ({
  data,
  className,
  title,
  rightSlot,
  beforeSlot,
  textareaMaxHeight,
  textareaMinHeight,
  showCopy = true,
  isLoading,
  contentProps,
}: Props) => {
  // https://bugs.chromium.org/p/chromium/issues/detail?id=1362573
  // there is a problem with scrollbar color in chromium
  // so blackAlpha.50 here is replaced with #f5f5f6
  // and whiteAlpha.50 is replaced with #1a1b1b
  // const bgColor = useColorModeValue('blackAlpha.50', 'whiteAlpha.50');
  const bgColor = { _light: '#f5f5f6', _dark: '#1a1b1b' };
  return (
    <Box className={ className } as="section" title={ title }>
      { (title || rightSlot || showCopy) && (
        <Flex justifyContent={ title ? 'space-between' : 'flex-end' } alignItems="center" mb={{ base: 1, lg: 3 }}>
          { title && <Skeleton fontWeight={ 500 } loading={ isLoading }>{ title }</Skeleton> }
          { rightSlot }
          { typeof data === 'string' && showCopy && <CopyToClipboard text={ data } isLoading={ isLoading }/> }
        </Flex>
      ) }
      { beforeSlot }
      <Skeleton
        p={ 4 }
        bgColor={ isLoading ? 'inherit' : bgColor }
        maxH={ textareaMaxHeight || '400px' }
        minH={ textareaMinHeight || (isLoading ? '200px' : undefined) }
        fontSize="sm"
        borderRadius="md"
        wordBreak="break-all"
        whiteSpace="pre-wrap"
        overflowX="hidden"
        overflowY="auto"
        loading={ isLoading }
        { ...contentProps }
      >
        { data }
      </Skeleton>
    </Box>
  );
};

export default React.memo(chakra(RawDataSnippet));
