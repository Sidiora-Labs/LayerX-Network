import { chakra, Flex } from '@chakra-ui/react';
import React from 'react';

import type { ItemsProps } from './types';
import type { SearchResultBlob } from 'types/api/search';

import * as BlobEntity from 'ui/shared/entities/blob/BlobEntity';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';

const SearchBarSuggestBlob = ({ data }: ItemsProps<SearchResultBlob>) => {
  return (
    <Flex alignItems="center" minW={ 0 }>
      <BlobEntity.Icon/>
      <chakra.mark overflow="hidden" whiteSpace="nowrap" fontWeight={ 700 }>
        <HashStringShortenDynamic hash={ data.blob_hash } noTooltip/>
      </chakra.mark>
    </Flex>
  );
};

export default React.memo(SearchBarSuggestBlob);
