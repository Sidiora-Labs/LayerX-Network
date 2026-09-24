import { Box, Flex, Text } from '@chakra-ui/react';
import { mapValues } from 'es-toolkit';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { TokenType } from 'types/api/token';
import type { ClusterChainConfig } from 'types/multichain';

import { route } from 'nextjs/routes';

import * as contract from 'lib/multichain/contract';
import shortenString from 'lib/shortenString';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';
import IconSvg from 'ui/shared/IconSvg';

import SearchResultListItem from '../SearchResultListItem';

interface Props {
  data: multichain.AggregatedTokenInfo;
  chain: ClusterChainConfig;
  isMobile?: boolean;
}

const SearchResultItemToken = ({ data, chain, isMobile }: Props) => {

  const isVerified = contract.isVerified({ chain_infos: mapValues(data.chain_infos, (chainInfo) => ({ ...chainInfo, is_contract: true, coin_balance: '0' })) });

  return (
    <SearchResultListItem
      href={ route({ pathname: '/token/[hash]', query: { hash: data.address_hash } }, { chain }) }
    >
      <Box w={{ base: '100%', lg: '200px' }}>
        <TokenEntity
          token={{
            address_hash: data.address_hash,
            icon_url: data.icon_url ?? null,
            name: data.name ?? 'Unnamed token',
            symbol: data.symbol ?? '',
            type: data.type as unknown as TokenType,
            reputation: null,
          }}
          chain={ chain }
          noLink
          jointSymbol
          noCopy
          fontWeight={{ base: '600', lg: '700' }}
        />
      </Box>
      <Flex alignItems="center" w={{ base: '100%', lg: 'auto' }} flexGrow={ 1 }>
        <Box
          overflow="hidden"
          whiteSpace="nowrap"
          textOverflow="ellipsis"
          fontWeight={{ base: '400', lg: '500' }}
          color="text.secondary"
          _groupHover={{ color: 'inherit' }}
        >
          { isMobile ? shortenString(data.address_hash) : (
            <HashStringShortenDynamic hash={ data.address_hash }/>
          ) }
        </Box>
        { isVerified && <IconSvg name="status/success" boxSize="14px" color="green.500" ml={ 1 } flexShrink={ 0 }/> }
        <Text
          overflow="hidden"
          whiteSpace="nowrap"
          textOverflow="ellipsis"
          fontWeight={{ base: '400', lg: '600' }}
          ml="auto"
          maxW={{ base: '60%', lg: 'unset' }}
        >
          { (data.type as string) === 'ERC-20' && data.exchange_rate && `$${ Number(data.exchange_rate).toLocaleString() }` }
          { (data.type as string) !== 'ERC-20' && data.total_supply && `Items ${ Number(data.total_supply).toLocaleString() }` }
        </Text>
      </Flex>
    </SearchResultListItem>
  );
};

export default React.memo(SearchResultItemToken);
