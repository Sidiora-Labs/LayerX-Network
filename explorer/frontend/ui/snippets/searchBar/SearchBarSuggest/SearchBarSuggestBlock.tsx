import { Text, Flex, Grid, Box } from '@chakra-ui/react';
import React from 'react';

import type { ItemsProps } from './types';
import type * as multichain from 'types/client/multichainAggregator';
import type { SearchResultBlock } from 'types/client/search';

import highlightText from 'lib/highlightText';
import { Tag } from 'toolkit/chakra/tag';
import * as BlockEntity from 'ui/shared/entities/block/BlockEntity';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';
import Time from 'ui/shared/time/Time';

const SearchBarSuggestBlock = ({ data, isMobile, searchTerm, chainInfo }: ItemsProps<SearchResultBlock | multichain.QuickSearchResultBlock>) => {
  const icon = <BlockEntity.Icon chain={ chainInfo }/>;
  const shouldHighlightHash = data.block_hash?.toLowerCase() === searchTerm.toLowerCase();
  const isFutureBlock = 'timestamp' in data && data.timestamp === undefined;
  const hasOnlyHash = data.block_number === undefined && data.block_hash !== undefined;

  if (hasOnlyHash) {
    const hash = (
      <Box
        overflow="hidden"
        whiteSpace="nowrap"
        as="mark"
        display="block"
      >
        <HashStringShortenDynamic hash={ data.block_hash } noTooltip/>
      </Box>
    );
    return (
      <Flex alignItems="center">
        { icon }
        { hash }
      </Flex>
    );
  }

  const blockNumber = (
    <Text
      fontWeight={ 700 }
      overflow="hidden"
      whiteSpace="nowrap"
      textOverflow="ellipsis"
    >
      <span dangerouslySetInnerHTML={{ __html: highlightText(data.block_number.toString(), searchTerm) }}/>
    </Text>
  );
  const hash = data.block_hash && !isFutureBlock ? (
    <Text
      color="text.secondary"
      overflow="hidden"
      whiteSpace="nowrap"
      as={ shouldHighlightHash ? 'mark' : 'span' }
      display="block"
    >
      <HashStringShortenDynamic hash={ data.block_hash } noTooltip/>
    </Text>
  ) : null;
  const date = 'timestamp' in data && data.timestamp && !isFutureBlock ? <Time timestamp={ data.timestamp } color="text.secondary" format="lll_s"/> : undefined;
  const futureBlockText = <Text color="text.secondary">Learn estimated time for this block to be created.</Text>;
  const blockType = 'block_type' in data ? data.block_type : undefined;

  if (isMobile) {
    return (
      <>
        <Flex alignItems="center">
          { icon }
          { blockNumber }
          { blockType === 'reorg' && <Tag ml="auto">Reorg</Tag> }
          { blockType === 'uncle' && <Tag ml="auto">Uncle</Tag> }
        </Flex>
        { hash }
        { isFutureBlock ? futureBlockText : date }
      </>
    );
  }

  return (
    <Grid templateColumns="228px minmax(auto, max-content) auto" gap={ 2 }>
      <Flex alignItems="center">
        { icon }
        { blockNumber }
      </Flex>
      <Flex columnGap={ 3 } minW={ 0 } alignItems="center">
        { blockType === 'reorg' && <Tag flexShrink={ 0 }>Reorg</Tag> }
        { blockType === 'uncle' && <Tag flexShrink={ 0 }>Uncle</Tag> }
        { isFutureBlock ? futureBlockText : hash }
      </Flex>
      { date && <Text color="text.secondary" textAlign="end">{ date }</Text> }
    </Grid>
  );
};

export default React.memo(SearchBarSuggestBlock);
