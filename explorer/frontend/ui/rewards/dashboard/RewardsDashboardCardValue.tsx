import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import { Heading } from 'toolkit/chakra/heading';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Hint } from 'toolkit/components/Hint/Hint';

import MeritsIcon from '../MeritsIcon';

type Props = {
  label?: string;
  value: number | string | undefined;
  withIcon?: boolean;
  hint?: string | React.ReactNode;
  isLoading?: boolean;
  bottomText?: string;
  isBottomTextLoading?: boolean;
};

const RewardsDashboardCardValue = ({ label, value, withIcon, hint, isLoading, bottomText, isBottomTextLoading }: Props) => (
  <Flex key={ label } flexDirection="column" alignItems="center" gap={{ base: 1, md: 2 }}>
    { label && (
      <Flex alignItems="center" gap={ 1 }>
        { hint && <Hint label={ hint }/> }
        <Text textStyle="xs" fontWeight="500" color="text.secondary">
          { label }
        </Text>
      </Flex>
    ) }
    <Skeleton
      loading={ isLoading }
      display="flex"
      alignItems="center"
      justifyContent="center"
      gap={ 2 }
      minW="100px"
    >
      { withIcon && <MeritsIcon boxSize={ 8 }/> }
      <Heading level="1">
        { value }
      </Heading>
    </Skeleton>
    { bottomText && (
      <Skeleton loading={ isBottomTextLoading || isLoading } minW="100px">
        <Text textStyle={{ base: 'xs', md: 'sm' }} color="text.secondary">
          { bottomText }
        </Text>
      </Skeleton>
    ) }
  </Flex>
);

export default RewardsDashboardCardValue;
