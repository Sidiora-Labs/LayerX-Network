import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { TChainIndicator } from './types';
import type { ChainIndicatorId } from 'types/homepage';

import ChainIndicatorItem from './ChainIndicatorItem';

interface Props {
  indicators: Array<TChainIndicator>;
  isLoading: boolean;
  selectedId: ChainIndicatorId;
  onItemClick: (id: ChainIndicatorId) => void;
}

const ChainIndicatorsList = ({ indicators, isLoading, selectedId, onItemClick }: Props) => {
  if (indicators.length < 2) {
    return null;
  }

  return (
    <Flex
      flexShrink={ 0 }
      flexDir="column"
      as="ul"
      borderRadius="lg"
      rowGap="6px"
      m={{ base: 'auto 0', lg: 0 }}
    >
      { indicators.map((indicator) => {
        return (
          <ChainIndicatorItem
            key={ indicator.id }
            indicator={ indicator }
            isSelected={ selectedId === indicator.id }
            onClick={ onItemClick }
            isLoading={ isLoading }
          />
        );
      }) }
    </Flex>
  );
};

export default React.memo(ChainIndicatorsList);
