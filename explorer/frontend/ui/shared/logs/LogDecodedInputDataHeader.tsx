import type { FlexProps } from '@chakra-ui/react';
import { Separator, Flex, VStack } from '@chakra-ui/react';
import React from 'react';

import { Badge } from 'toolkit/chakra/badge';
import { Skeleton } from 'toolkit/chakra/skeleton';

interface Props {
  methodId: string;
  methodCall: string;
  isLoading?: boolean;
  rightSlot?: React.ReactNode;
}

interface ItemProps extends FlexProps {
  label: string;
  children: React.ReactNode;
  isLoading?: boolean;
}

const Item = ({ label, children, isLoading, ...rest }: ItemProps) => {
  return (
    <Flex
      w="100%"
      columnGap={{ base: 2, lg: 5 }}
      rowGap={ 2 }
      px={{ base: 0, lg: 4 }}
      flexDir={{ base: 'column', lg: 'row' }}
      alignItems={{ base: 'flex-start', lg: 'center' }}
      { ...rest }
    >
      <Skeleton fontWeight={ 600 } w={{ base: 'auto', lg: '80px' }} flexShrink={ 0 } loading={ isLoading }>
        { label }
      </Skeleton >
      { children }
    </Flex>
  );
};

const LogDecodedInputDataHeader = ({ methodId, methodCall, isLoading, rightSlot }: Props) => {
  return (
    <VStack
      align="flex-start"
      separator={ <Separator w="100%" borderColor="border.divider"/> }
      textStyle="sm"
      flexGrow={ 1 }
      gap={ 2 }
      w="100%"
    >
      <Flex columnGap={ 2 } w="100%">
        <Item label="Method id" isLoading={ isLoading } flexDir="row" alignItems="center">
          <Badge loading={ isLoading }>{ methodId }</Badge>
        </Item>
        { rightSlot }
      </Flex>
      <Item label="Call" isLoading={ isLoading }>
        <Skeleton loading={ isLoading } whiteSpace="pre-wrap" wordBreak="break-all" flexGrow={ 1 }>{ methodCall }</Skeleton>
      </Item>
    </VStack>
  );
};

export default LogDecodedInputDataHeader;
