import { Grid } from '@chakra-ui/react';
import React from 'react';

import type { NFTTokenType } from 'types/api/token';

import { useMultichainContext } from 'lib/contexts/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import Pagination from 'ui/shared/pagination/Pagination';
import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';

import AddressNftTypeFilter from './AddressNftTypeFilter';
import NFTItem from './NFTItem';

type Props = {
  tokensQuery: QueryWithPagesResult<'general:address_nfts'>;
  tokenTypes: Array<NFTTokenType> | undefined;
  onTokenTypesChange: (value: Array<NFTTokenType>) => void;
};

const AddressNFTs = ({ tokensQuery, tokenTypes, onTokenTypesChange }: Props) => {
  const isMobile = useIsMobile();
  const multichainContext = useMultichainContext();

  const { isError, isPlaceholderData, data, pagination } = tokensQuery;

  const hasActiveFilters = Boolean(tokenTypes?.length);

  const actionBar = isMobile && pagination.isVisible && (
    <ActionBar mt={ -6 }>
      <AddressNftTypeFilter value={ tokenTypes } onChange={ onTokenTypesChange }/>
      <Pagination ml="auto" { ...pagination }/>
    </ActionBar>
  );

  const content = data?.items ? (
    <Grid
      w="100%"
      columnGap={{ base: 3, lg: 6 }}
      rowGap={{ base: 3, lg: 6 }}
      gridTemplateColumns={{ base: 'repeat(2, calc((100% - 12px)/2))', lg: 'repeat(auto-fill, minmax(210px, 1fr))' }}
    >
      { data.items.map((item, index) => {
        const key = item.token.address_hash + '_' + (item.id && !isPlaceholderData ? `id_${ item.id }` : `index_${ index }`);

        return (
          <NFTItem
            key={ key }
            { ...item }
            isLoading={ isPlaceholderData }
            withTokenLink
            chain={ multichainContext?.chain }
          />
        );
      }) }
    </Grid>
  ) : null;

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

export default AddressNFTs;
