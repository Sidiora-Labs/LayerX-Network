import { Box, Flex } from '@chakra-ui/react';
import React from 'react';
import { useInView } from 'react-intersection-observer';

import type * as multichain from '@blockscout/multichain-aggregator-types';

import multichainConfig from 'configs/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';

import SearchResultItemAddress from './items/SearchResultItemAddress';
import SearchResultItemBlock from './items/SearchResultItemBlock';
import SearchResultItemBlockNumber from './items/SearchResultItemBlockNumber';
import SearchResultItemDomain from './items/SearchResultItemDomain';
import SearchResultItemToken from './items/SearchResultItemToken';
import SearchResultItemTx from './items/SearchResultItemTx';
import type { QueryType, SearchQueries } from './utils';

interface Props<T extends QueryType> {
  queryType: T;
  query: SearchQueries[T];
  maxItems?: number;
}

const SearchResultsList = <T extends QueryType>({ queryType, query, maxItems = Infinity }: Props<T>) => {
  const config = multichainConfig();
  const isMobile = useIsMobile();
  const { ref: cutRef } = useInView({
    triggerOnce: false,
    skip: maxItems !== Infinity || query.isFetchingNextPage || !query.hasNextPage,
    onChange: (inView) => {
      if (inView) {
        query.fetchNextPage();
      }
    },
  });

  return (
    <Flex flexDir="column" textStyle="sm">
      { query.data?.pages?.slice(0, maxItems !== Infinity ? 1 : Infinity).map((page, index) => (
        <React.Fragment key={ index }>
          { page.items.slice(0, maxItems).map((item) => {

            const chain = 'chain_id' in item ? config?.chains.find((chain) => chain.id === item.chain_id) : undefined;

            switch (queryType) {
              case 'tokens':
              case 'nfts': {
                const chain = 'chain_infos' in item ? config?.chains.find((chain) => chain.id === Object.keys(item.chain_infos)[0]) : undefined;
                if (!chain) {
                  return null;
                }
                const token = item as multichain.AggregatedTokenInfo;
                return <SearchResultItemToken key={ token.address_hash + chain.id } data={ token } chain={ chain } isMobile={ isMobile }/>;
              }
              case 'blockNumbers': {
                if (!chain) {
                  return null;
                }
                const block = item as multichain.ChainBlockNumber;
                return <SearchResultItemBlockNumber key={ block.block_number + String(chain.id) } data={ block } chain={ chain }/>;
              }
              case 'blocks': {
                if (!chain) {
                  return null;
                }
                const block = item as multichain.Hash;
                return <SearchResultItemBlock key={ block.hash + chain.id } data={ block } chain={ chain }/>;
              }
              case 'transactions': {
                if (!chain) {
                  return null;
                }
                const block = item as multichain.Hash;
                return <SearchResultItemTx key={ block.hash + chain.id } data={ block } chain={ chain }/>;
              }
              case 'addresses': {
                const address = item as multichain.GetAddressResponse;
                return <SearchResultItemAddress key={ address.hash } data={ address } isMobile={ isMobile }/>;
              }
              case 'domains': {
                const domain = item as multichain.Domain;
                return <SearchResultItemDomain key={ domain.address } data={ domain }/>;
              }
              default:
                return null;
            }
          }) }
        </React.Fragment>
      )) }

      { query.isFetching && <ContentLoader maxW="240px" mt={ 6 }/> }

      <Box h="0" w="100px" ref={ cutRef }/>
    </Flex>
  );
};

export default SearchResultsList;
