import { Box, Flex, Text, Grid, HStack } from '@chakra-ui/react';
import React from 'react';

import type { NFTTokenType } from 'types/api/token';

import { route } from 'nextjs/routes';

import { useMultichainContext } from 'lib/contexts/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import NftFallback from 'ui/shared/nft/NftFallback';
import Pagination from 'ui/shared/pagination/Pagination';
import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';

import AddressNftTypeFilter from './AddressNftTypeFilter';
import NFTItem from './NFTItem';
import NFTItemContainer from './NFTItemContainer';

type Props = {
  collectionsQuery: QueryWithPagesResult<'general:address_collections'>;
  address: string;
  tokenTypes: Array<NFTTokenType> | undefined;
  onTokenTypesChange: (value: Array<NFTTokenType>) => void;
};

const AddressCollections = ({ collectionsQuery, address, tokenTypes, onTokenTypesChange }: Props) => {
  const isMobile = useIsMobile();
  const multichainContext = useMultichainContext();

  const { isError, isPlaceholderData, data, pagination } = collectionsQuery;

  const hasActiveFilters = Boolean(tokenTypes?.length);

  const actionBar = isMobile && pagination.isVisible && (
    <ActionBar mt={ -6 }>
      <AddressNftTypeFilter value={ tokenTypes } onChange={ onTokenTypesChange }/>
      <Pagination ml="auto" { ...pagination }/>
    </ActionBar>
  );

  const content = data?.items ? data?.items.filter((item) => item.token_instances.length > 0).map((item, index) => {
    const collectionUrl = route({
      pathname: '/token/[hash]',
      query: {
        hash: item.token.address_hash,
        tab: 'inventory',
        holder_address_hash: address,
        scroll_to_tabs: 'true',
      },
    }, { chain: multichainContext?.chain });
    const hasOverload = Number(item.amount) > item.token_instances.length;
    return (
      <Box key={ item.token.address_hash + index } mb={ 6 }>
        <Flex mb={ 3 } flexWrap="wrap" lineHeight="30px">
          <TokenEntity
            width="auto"
            noSymbol
            token={ item.token }
            isLoading={ isPlaceholderData }
            noCopy
            fontWeight="600"
            chain={ multichainContext?.chain }
          />
          <Skeleton loading={ isPlaceholderData } mr={ 3 }>
            <Text color="text.secondary" whiteSpace="pre">{ ` - ${ Number(item.amount).toLocaleString() } item${ Number(item.amount) > 1 ? 's' : '' }` }</Text>
          </Skeleton>
          <Link href={ collectionUrl } loading={ isPlaceholderData }>
            View in collection
          </Link>
        </Flex>
        <Grid
          w="100%"
          mb={ 6 }
          columnGap={{ base: 3, lg: 6 }}
          rowGap={{ base: 3, lg: 6 }}
          gridTemplateColumns={{ base: 'repeat(2, calc((100% - 12px)/2))', lg: 'repeat(auto-fill, minmax(210px, 1fr))' }}
        >
          { item.token_instances.map((instance, index) => {
            const key = item.token.address_hash + '_' + (instance.id && !isPlaceholderData ? `id_${ instance.id }` : `index_${ index }`);

            return (
              <NFTItem
                key={ key }
                { ...instance }
                token={ item.token }
                isLoading={ isPlaceholderData }
                chain={ multichainContext?.chain }
              />
            );
          }) }
          { hasOverload && (
            <Link href={ collectionUrl }>
              <NFTItemContainer display="flex" alignItems="center" justifyContent="center" flexDirection="column" minH="248px">
                <HStack gap={ 2 } mb={ 3 }>
                  <NftFallback bgColor={{ _light: 'unset', _dark: 'unset' }} w="30px" h="30px" boxSize="30px" p={ 0 }/>
                  <NftFallback bgColor={{ _light: 'unset', _dark: 'unset' }} w="30px" h="30px" boxSize="30px" p={ 0 }/>
                  <NftFallback bgColor={{ _light: 'unset', _dark: 'unset' }} w="30px" h="30px" boxSize="30px" p={ 0 }/>
                </HStack>
                View all NFTs
              </NFTItemContainer>
            </Link>
          ) }
        </Grid>
      </Box>
    );
  }) : null;

  return (
    <DataListDisplay
      isError={ isError }
      itemsNum={ data?.items?.length }
      emptyText="There are no tokens of selected type."
      actionBar={ actionBar }
      hasActiveFilters={ hasActiveFilters }
      emptyStateProps={{
        term: 'token',
      }}
    >
      { content }
    </DataListDisplay>
  );
};

export default AddressCollections;
