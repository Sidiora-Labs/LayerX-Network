import { Text, Flex, Box } from '@chakra-ui/react';
import React from 'react';

import type { TChainIndicator } from './types';
import type { ChainIndicatorId } from 'types/homepage';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { mdash } from 'toolkit/utils/htmlEntities';
interface Props {
  indicator: TChainIndicator;
  isSelected: boolean;
  onClick: (id: ChainIndicatorId) => void;
  isLoading: boolean;
}

const ChainIndicatorItem = ({ indicator, isSelected, onClick, isLoading }: Props) => {
  const handleClick = React.useCallback(() => {
    onClick(indicator.id);
  }, [ indicator.id, onClick ]);

  const valueContent = (() => {
    if (indicator.value.includes('N/A')) {
      return <Text opacity="control.disabled" fontWeight={ 400 }>{ mdash }</Text>;
    }

    return (
      <Skeleton loading={ isLoading } fontWeight={ 600 } minW="30px">
        { indicator.value }
      </Skeleton>
    );
  })();

  const valueDiffContent = (() => {
    if (indicator.valueDiff === undefined || (!isLoading && indicator.value.includes('N/A'))) {
      return null;
    }

    const diffColor = indicator.valueDiff >= 0 ? 'green.500' : 'red.500';

    return (
      <Skeleton loading={ isLoading } ml={ 1 } display="flex" alignItems="center" color={ diffColor }>
        <span>{ indicator.valueDiff >= 0 ? '+' : '-' }</span>
        <Text color={ diffColor } fontWeight={ 600 }>{ Math.abs(indicator.valueDiff) }%</Text>
      </Skeleton>
    );
  })();

  return (
    <Flex
      alignItems="center"
      columnGap={ 2 }
      flexGrow={{ base: 0, lg: 1 }}
      px={{ base: '6px', lg: 2 }}
      py="6px"
      as="li"
      borderRadius="base"
      cursor="pointer"
      color={ isSelected ? 'text.secondary' : 'link.primary' }
      bgColor={ isSelected ? 'bg.primary' : undefined }
      onClick={ handleClick }
      fontSize="xs"
      fontWeight={ 500 }
      _hover={{
        bgColor: 'bg.primary',
        color: isSelected ? 'text.secondary' : 'hover',
        zIndex: 1,
      }}
    >
      { indicator.icon }
      <Box display={{ base: 'none', lg: 'block' }}>
        <span>{ indicator.titleShort || indicator.title }</span>
        <Flex alignItems="center" color="text.primary">
          { valueContent }
          { valueDiffContent }
        </Flex>
      </Box>
    </Flex>
  );
};

export default React.memo(ChainIndicatorItem);
