import { chakra, Text, Flex } from '@chakra-ui/react';
import React from 'react';

import type { ItemsProps } from './types';
import type { SearchResultTx } from 'types/api/search';
import type * as multichain from 'types/client/multichainAggregator';

import * as TxEntity from 'ui/shared/entities/tx/TxEntity';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';
import Time from 'ui/shared/time/Time';

const SearchBarSuggestTx = ({ data, isMobile, chainInfo }: ItemsProps<SearchResultTx | multichain.QuickSearchResultTransaction>) => {
  const icon = <TxEntity.Icon chain={ chainInfo }/>;
  const hash = (
    <chakra.mark overflow="hidden" whiteSpace="nowrap" fontWeight={ 700 }>
      <HashStringShortenDynamic hash={ data.transaction_hash } noTooltip/>
    </chakra.mark>
  );
  const date = 'timestamp' in data && data.timestamp ? <Time timestamp={ data.timestamp } format="lll_s"/> : undefined;

  if (isMobile) {
    return (
      <>
        <Flex alignItems="center">
          { icon }
          { hash }
        </Flex>
        <Text color="text.secondary">{ date }</Text>
      </>
    );
  }

  return (
    <Flex columnGap={ 2 }>
      <Flex alignItems="center" minW={ 0 }>
        { icon }
        { hash }
      </Flex>
      <Text color="text.secondary" textAlign="end" flexShrink={ 0 } ml="auto">{ date }</Text>
    </Flex>
  );
};

export default React.memo(SearchBarSuggestTx);
