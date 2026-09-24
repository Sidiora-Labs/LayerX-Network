import type { BoxProps } from '@chakra-ui/react';
import { chakra, Flex } from '@chakra-ui/react';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { Hint } from 'toolkit/components/Hint/Hint';

interface Props {
  label: string;
  children: React.ReactNode;
  isLoading?: boolean;
  hint?: string;
  contentProps?: BoxProps;
}

const ContractDetailsInfoItem = ({ label, children, isLoading, hint, contentProps }: Props) => {
  return (
    <>
      <Skeleton loading={ isLoading } flexShrink={ 0 } fontWeight={ 500 }>
        <Flex alignItems="center">
          { label }
          { hint && <Hint label={ hint } ml={ 2 }/> }
        </Flex>
      </Skeleton>
      <Skeleton loading={ isLoading } wordBreak="break-all" maxW="100%" overflow="hidden" { ...contentProps }>{ children }</Skeleton>
    </>
  );
};

export default React.memo(chakra(ContractDetailsInfoItem));
