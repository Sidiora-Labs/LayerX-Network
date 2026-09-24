import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import type { CctxListItem } from '@blockscout/zetachain-cctx-types';

import { route } from 'nextjs/routes';

import { SECOND } from 'toolkit/utils/consts';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';
import IconSvg from 'ui/shared/IconSvg';
import Time from 'ui/shared/time/Time';

import SearchBarSuggestItemLink from './SearchBarSuggestItemLink';

interface Props {
  data: CctxListItem;
  isMobile: boolean | undefined;
  searchTerm: string;
  onClick: (event: React.MouseEvent<HTMLAnchorElement>) => void;
}

const SearchBarSuggestZetaChainCCTX = ({ data, isMobile, searchTerm, onClick }: Props) => {
  const icon = <IconSvg name="interop" boxSize={ 5 } marginRight={ 1 } color="text.secondary"/>;

  // search term can be either cctx hash or observed hash (hash from another chain)
  const hash = (
    <chakra.span as={ searchTerm === data.index ? 'mark' : 'span' } overflow="hidden" whiteSpace="nowrap" fontWeight={ 700 }>
      <HashStringShortenDynamic hash={ data.index } noTooltip/>
    </chakra.span>
  );

  let content;

  if (isMobile) {
    content = (
      <>
        <Flex alignItems="center">
          { icon }
          { hash }
        </Flex>
        <Time timestamp={ Number(data.last_update_timestamp) * SECOND } color="text.secondary" format="lll_s"/>
      </>
    );
  } else {
    content = (
      <Flex columnGap={ 2 }>
        <Flex alignItems="center" minW={ 0 }>
          { icon }
          { hash }
        </Flex>
        <Time timestamp={ Number(data.last_update_timestamp) * SECOND } color="text.secondary" textAlign="end" flexShrink={ 0 } ml="auto" format="lll_s"/>
      </Flex>
    );
  }

  return (
    <SearchBarSuggestItemLink href={ route({ pathname: '/cc/tx/[hash]', query: { hash: data.index } }) } onClick={ onClick }>
      { content }
    </SearchBarSuggestItemLink>
  );
};

export default React.memo(SearchBarSuggestZetaChainCCTX);
