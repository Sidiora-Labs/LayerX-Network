import { chakra, Flex } from '@chakra-ui/react';
import React from 'react';

import type { ItemsProps } from './types';
import type { SearchResultUserOp } from 'types/api/search';

import * as UserOpEntity from 'ui/shared/entities/userOp/UserOpEntity';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';
import Time from 'ui/shared/time/Time';

const SearchBarSuggestUserOp = ({ data, isMobile }: ItemsProps<SearchResultUserOp>) => {
  const icon = <UserOpEntity.Icon/>;
  const hash = (
    <chakra.mark overflow="hidden" whiteSpace="nowrap" fontWeight={ 700 }>
      <HashStringShortenDynamic hash={ data.user_operation_hash } noTooltip/>
    </chakra.mark>
  );

  if (isMobile) {
    return (
      <>
        <Flex alignItems="center">
          { icon }
          { hash }
        </Flex>
        <Time timestamp={ data.timestamp } color="text.secondary" format="lll_s"/>
      </>
    );
  }

  return (
    <Flex columnGap={ 2 }>
      <Flex alignItems="center" minW={ 0 }>
        { icon }
        { hash }
      </Flex>
      <Time timestamp={ data.timestamp } color="text.secondary" textAlign="end" flexShrink={ 0 } ml="auto" format="lll_s"/>
    </Flex>
  );
};

export default React.memo(SearchBarSuggestUserOp);
