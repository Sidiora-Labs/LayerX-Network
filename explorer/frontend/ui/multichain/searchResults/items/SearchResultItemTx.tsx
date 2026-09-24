import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { ClusterChainConfig } from 'types/multichain';

import { route } from 'nextjs/routes';

import TxEntity from 'ui/shared/entities/tx/TxEntity';

import SearchResultListItem from '../SearchResultListItem';

interface Props {
  data: multichain.Hash;
  chain: ClusterChainConfig;
}

const SearchResultItemTx = ({ data, chain }: Props) => {
  return (
    <SearchResultListItem
      href={ route({ pathname: '/tx/[hash]', query: { hash: String(data.hash) } }, { chain }) }
    >
      <TxEntity
        hash={ data.hash }
        chain={ chain }
        noLink
        noCopy
        fontWeight="700"
        w="100%"
      />
    </SearchResultListItem>
  );
};

export default React.memo(SearchResultItemTx);
