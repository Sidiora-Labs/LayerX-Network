import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { Hint } from 'toolkit/components/Hint/Hint';
import { mdash } from 'toolkit/utils/htmlEntities';
import FallbackChart from 'ui/shared/fallbacks/FallbackChart';
import IconSvg from 'ui/shared/IconSvg';

import ChainIndicatorChartContainer from './ChainIndicatorChartContainer';
import type { UseFetchChartDataResult } from './useChartDataQuery';

interface Props {
  isLoading: boolean;
  value: string;
  valueDiff?: number;
  chartQuery: UseFetchChartDataResult;
  title: string;
  hint?: string;
}

const ChainIndicatorsChart = ({ isLoading: isLoadingProp, value, valueDiff, chartQuery, title, hint }: Props) => {
  const isLoading = isLoadingProp || chartQuery.isPending;

  const valueTitleElement = (() => {
    if (isLoading) {
      return <Skeleton loading h="36px" w="200px"/>;
    }

    if (value.includes('N/A')) {
      return <Text fontWeight={ 700 } fontSize="30px" lineHeight="36px" opacity="control.disabled">{ mdash }</Text>;
    }

    return (
      <Text fontWeight={ 700 } fontSize="30px" lineHeight="36px">
        { value }
      </Text>
    );
  })();

  const valueDiffElement = (() => {
    if (valueDiff === undefined || (!isLoading && value.includes('N/A'))) {
      return null;
    }

    const diffColor = valueDiff >= 0 ? 'green.500' : 'red.500';

    return (
      <Skeleton loading={ isLoading } display="flex" alignItems="center" color={ diffColor } ml={ 2 }>
        <IconSvg name="arrows/up-head" boxSize={ 5 } mr={ 1 } transform={ valueDiff < 0 ? 'rotate(180deg)' : 'rotate(0)' }/>
        <Text color={ diffColor } fontWeight={ 600 }>{ valueDiff }%</Text>
      </Skeleton>
    );
  })();

  if (chartQuery.isError) {
    return <FallbackChart term={ title } h={{ base: '144px', lg: '184px' }}/>;
  }

  return (
    <Flex flexGrow={ 1 } flexDir="column">
      <Skeleton loading={ isLoading } display="flex" alignItems="center" w="fit-content" columnGap={ 1 }>
        <Text fontWeight={ 500 }>{ title }</Text>
        { hint && <Hint label={ hint }/> }
      </Skeleton>
      <Flex mb={{ base: 0, lg: 2 }} mt={ 1 } alignItems="end">
        { valueTitleElement }
        { valueDiffElement }
      </Flex>
      <Flex h={{ base: '80px', lg: '110px' }} alignItems="flex-start" flexGrow={ 1 }>
        <ChainIndicatorChartContainer { ...chartQuery } isPending={ isLoading }/>
      </Flex>
    </Flex>
  );
};

export default React.memo(ChainIndicatorsChart);
