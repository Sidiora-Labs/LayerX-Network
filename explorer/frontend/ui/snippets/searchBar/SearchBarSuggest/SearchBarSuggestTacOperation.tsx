import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import type { ItemsProps } from './types';
import type { SearchResultTacOperation } from 'types/api/search';

import * as OperationEntity from 'ui/shared/entities/operation/OperationEntity';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';
import TacOperationStatus from 'ui/shared/statusTag/TacOperationStatus';
import Time from 'ui/shared/time/Time';

const SearchBarSuggestTacOperation = ({ data, isMobile }: ItemsProps<SearchResultTacOperation>) => {
  const icon = <OperationEntity.Icon type={ data.tac_operation.type }/>;
  const hash = (
    <chakra.mark overflow="hidden" whiteSpace="nowrap" fontWeight={ 700 } mr={ 2 }>
      <HashStringShortenDynamic hash={ data.tac_operation.operation_id } noTooltip/>
    </chakra.mark>
  );
  const status = <TacOperationStatus status={ data.tac_operation.type }/>;

  if (isMobile) {
    return (
      <>
        <Flex alignItems="center">
          { icon }
          { hash }
          { status }
        </Flex>
        <Time timestamp={ data.tac_operation.timestamp } color="text.secondary" format="lll_s"/>
      </>
    );
  }

  return (
    <Flex columnGap={ 2 }>
      <Flex alignItems="center" minW={ 0 }>
        { icon }
        { hash }
        { status }
      </Flex>
      <Time timestamp={ data.tac_operation.timestamp } color="text.secondary" textAlign="end" flexShrink={ 0 } ml="auto" format="lll_s"/>
    </Flex>
  );
};

export default React.memo(SearchBarSuggestTacOperation);
