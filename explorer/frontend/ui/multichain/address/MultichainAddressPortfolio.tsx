import { Box, HStack } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import { MultichainProvider } from 'lib/contexts/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import getQueryParamString from 'lib/router/getQueryParamString';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import AddressCollections from 'ui/address/tokens/AddressCollections';
import AddressNftDisplayTypeRadio from 'ui/address/tokens/AddressNftDisplayTypeRadio';
import AddressNFTs from 'ui/address/tokens/AddressNFTs';
import AddressNftTypeFilter from 'ui/address/tokens/AddressNftTypeFilter';
import useAddressNftQuery from 'ui/address/tokens/useAddressNftQuery';
import ChainSelect from 'ui/multichain/components/ChainSelect';
import Pagination from 'ui/shared/pagination/Pagination';

import getAvailableChainIds from './getAvailableChainIds';
import MultichainAddressPortfolioTokens from './portfolio/MultichainAddressPortfolioTokens';

export const ADDRESS_OP_PORTFOLIO_TAB_IDS = [ 'portfolio_tokens' as const, 'portfolio_nfts' as const ];
const TABS_RIGHT_SLOT_PROPS = {
  display: 'flex',
  justifyContent: { base: 'flex-end', lg: 'flex-start' },
  ml: { base: 0, lg: 6 },
  widthAllocation: 'available' as const,
};
const TAB_LIST_PROPS = {
  marginBottom: 0,
  pt: 6,
  pb: 2,
  marginTop: -6,
};
const TABS_PRESERVED_PARAMS = [ 'chain_id' ];

interface Props {
  addressData: multichain.GetAddressResponse | undefined;
  isLoading: boolean;
}

const MultichainAddressPortfolio = ({ addressData, isLoading }: Props) => {
  const scrollRef = React.useRef<HTMLDivElement>(null);

  const isMobile = useIsMobile();
  const router = useRouter();

  const tab = getQueryParamString(router.query.tab) as typeof ADDRESS_OP_PORTFOLIO_TAB_IDS[number] | 'portfolio' | undefined;
  const hash = getQueryParamString(router.query.hash);
  const chainIds = React.useMemo(() => getAvailableChainIds(addressData), [ addressData ]);

  const { nftsQuery, collectionsQuery, displayType: nftDisplayType, tokenTypes: nftTokenTypes, onDisplayTypeChange, onTokenTypesChange } = useAddressNftQuery({
    scrollRef,
    enabled: !isLoading && tab === 'portfolio_nfts' && chainIds.length > 0,
    addressHash: hash,
    isMultichain: true,
    chainIds,
  });

  const handleChainChange = React.useCallback((chainId: string | null) => {
    const chainValue = chainId ? [ chainId ] : [ chainIds[0] ];
    nftsQuery.onChainValueChange({ value: chainValue });
    collectionsQuery.onChainValueChange({ value: chainValue });
  }, [ nftsQuery, collectionsQuery, chainIds ]);

  const hasActiveFilters = (() => {
    if (tab === 'portfolio_nfts') {
      return Boolean(nftTokenTypes?.length);
    }

    return false;
  })();

  const rightSlot = (() => {
    if (tab === 'portfolio_nfts') {
      const query = nftDisplayType === 'list' ? nftsQuery : collectionsQuery;
      const chainSelect = (
        <ChainSelect
          loading={ query.pagination.isLoading }
          value={ query.chainValue }
          onValueChange={ query.onChainValueChange }
          chainIds={ chainIds }
        />
      );

      const hasData =
        (!nftsQuery.isPlaceholderData && nftsQuery.data?.items.length) ||
        (!collectionsQuery.isPlaceholderData && collectionsQuery.data?.items.length);

      return (
        <>
          <HStack gap={ 2 }>
            { (hasData || hasActiveFilters) && !(isMobile && query.pagination.isVisible) &&
              <AddressNftTypeFilter value={ nftTokenTypes } onChange={ onTokenTypesChange }/> }
            { (hasData || hasActiveFilters) && isMobile &&
                <AddressNftDisplayTypeRadio value={ nftDisplayType } onChange={ onDisplayTypeChange } ml={{ base: 0, lg: 6 }}/> }
            { chainSelect }
          </HStack>
          { (hasData || hasActiveFilters) && !isMobile && <AddressNftDisplayTypeRadio value={ nftDisplayType } onChange={ onDisplayTypeChange } ml={ 6 }/> }
          { query.pagination.isVisible && !isMobile && <Pagination { ...query.pagination } ml="auto"/> }
        </>
      );
    }
  })();

  const tabs: Array<TabItemRegular> = [
    {
      id: 'portfolio_tokens',
      title: 'Tokens',
      component: <MultichainAddressPortfolioTokens addressData={ addressData } isLoading={ isLoading } onChainChange={ handleChainChange }/>,
    },
    {
      id: 'portfolio_nfts',
      title: 'NFT',
      component: nftDisplayType === 'list' ? (
        <MultichainProvider chainId={ nftsQuery.chainValue?.[0] }>
          <AddressNFTs tokensQuery={ nftsQuery } tokenTypes={ nftTokenTypes } onTokenTypesChange={ onTokenTypesChange }/>
        </MultichainProvider>
      ) : (
        <MultichainProvider chainId={ collectionsQuery.chainValue?.[0] }>
          <AddressCollections collectionsQuery={ collectionsQuery } address={ hash } tokenTypes={ nftTokenTypes } onTokenTypesChange={ onTokenTypesChange }/>
        </MultichainProvider>
      ),
    },
  ];

  return (
    <>
      <Box ref={ scrollRef }/>
      <RoutedTabs
        variant="secondary"
        size="sm"
        tabs={ tabs }
        rightSlot={ rightSlot }
        rightSlotProps={ TABS_RIGHT_SLOT_PROPS }
        listProps={ isMobile ? undefined : TAB_LIST_PROPS }
        stickyEnabled={ !isMobile }
        preservedParams={ TABS_PRESERVED_PARAMS }
      />
    </>
  );
};

export default React.memo(MultichainAddressPortfolio);
