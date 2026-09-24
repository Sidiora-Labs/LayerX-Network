import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { ClusterChainConfig } from 'types/multichain';

import { route } from 'nextjs/routes';

import BlockEntity from 'ui/shared/entities/block/BlockEntity';

import SearchResultListItem from '../SearchResultListItem';

interface Props {
  data: multichain.Hash;
  chain: ClusterChainConfig;
}

const SearchResultItemBlock = ({ data, chain }: Props) => {
  return (
    <SearchResultListItem
      href={ route({ pathname: '/block/[height_or_hash]', query: { height_or_hash: String(data.hash) } }, { chain }) }
    >
      <BlockEntity
        number={ String(data.hash) }
        chain={ chain }
        noLink
        noCopy
        fontWeight={{ base: '600', lg: '700' }}
        w="100%"
      />
    </SearchResultListItem>
  );
};

export default React.memo(SearchResultItemBlock);
