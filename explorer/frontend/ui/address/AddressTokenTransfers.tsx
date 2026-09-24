import { HStack } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import useIsMounted from 'lib/hooks/useIsMounted';
import getQueryParamString from 'lib/router/getQueryParamString';
import { INTERCHAIN_TRANSFER } from 'stubs/interchainIndexer';
import { generateListStub } from 'stubs/utils';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import TokenTransfersCrossChainContent from 'ui/crossChain/transfers/TokenTransfersCrossChainContent';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import TokenTransferFilter from 'ui/shared/TokenTransfer/TokenTransferFilter';

import AddressAdvancedFilterLink from './AddressAdvancedFilterLink';
import AddressCsvExportLink from './AddressCsvExportLink';
import AddressTokenTransfersLocal from './AddressTokenTransfersLocal';
import useAddressTokenTransfersQuery from './useAddressTokenTransfersQuery';

export const ADDRESS_TOKEN_TRANSFERS_TAB_IDS = [ 'token_transfers_local' as const, 'token_transfers_cross_chain' as const ];
const TAB_LIST_PROPS = {
  marginBottom: 0,
  pt: 6,
  pb: 3,
  marginTop: -6,
};

interface Props {
  shouldRender?: boolean;
  isQueryEnabled?: boolean;
  // for tests only
  overloadCount?: number;
}

const AddressTokenTransfers = ({ shouldRender = true, overloadCount, isQueryEnabled = true }: Props) => {
  const router = useRouter();
  const isMounted = useIsMounted();
  const isMobile = useIsMobile();
  const hash = getQueryParamString(router.query.hash);
  const tab = getQueryParamString(router.query.tab) as typeof ADDRESS_TOKEN_TRANSFERS_TAB_IDS[number] | 'token_transfers' | undefined;

  const isLocalTab = tab === 'token_transfers_local' || tab === 'token_transfers';

  const localQuery = useAddressTokenTransfersQuery({
    currentAddress: hash,
    enabled: isQueryEnabled && isLocalTab,
  });

  const crossChainQuery = useQueryWithPages({
    resourceName: 'interchainIndexer:address_transfers',
    pathParams: { hash },
    options: {
      placeholderData: generateListStub<'interchainIndexer:address_transfers'>(INTERCHAIN_TRANSFER, 50, { next_page_params: undefined }),
      enabled: isQueryEnabled && !isLocalTab,
    },
  });

  const handleTabValueChange = React.useCallback(({ value }: { value: string }) => {
    if (value === 'token_transfers_local') {
      localQuery.setFilters({ type: [], filter: undefined });
    }
  }, [ localQuery ]);

  if (!isMounted || !shouldRender) {
    return null;
  }

  const tabs = [
    {
      id: [ 'token_transfers_local', 'token_transfers' ],
      title: 'Transfers',
      component: (
        <AddressTokenTransfersLocal
          query={ localQuery.query }
          filters={ localQuery.filters }
          onTypeFilterChange={ localQuery.onTypeFilterChange }
          onAddressFilterChange={ localQuery.onAddressFilterChange }
          addressHash={ hash }
          overloadCount={ overloadCount }
        />
      ),
    },
    config.features.crossChainTxs.isEnabled && {
      id: 'token_transfers_cross_chain',
      title: 'Cross-chain transfers',
      component: (
        <>
          { isMobile && crossChainQuery.pagination.isVisible && (
            <ActionBar>
              <Pagination ml="auto" { ...crossChainQuery.pagination }/>
            </ActionBar>
          ) }
          <TokenTransfersCrossChainContent
            items={ crossChainQuery.data?.items }
            isLoading={ crossChainQuery.isPlaceholderData }
            isError={ crossChainQuery.isError }
            pagination={ crossChainQuery.pagination }
            tableTop={ ACTION_BAR_HEIGHT_DESKTOP }
            currentAddress={ hash }
          />
        </>
      ),
    },
  ].filter(Boolean);

  const rightSlot = (() => {
    if (isLocalTab) {
      if (isMobile) {
        return null;
      }

      const numActiveFilters = (localQuery.filters.type?.length || 0) + (localQuery.filters.filter ? 1 : 0);

      return (
        <>
          <HStack gap={ 2 }>
            <TokenTransferFilter
              defaultTypeFilters={ localQuery.filters.type }
              onTypeFilterChange={ localQuery.onTypeFilterChange }
              appliedFiltersNum={ numActiveFilters }
              withAddressFilter
              onAddressFilterChange={ localQuery.onAddressFilterChange }
              defaultAddressFilter={ localQuery.filters.filter }
              isLoading={ localQuery.query.isPlaceholderData }
            />
          </HStack>
          <HStack gap={{ base: 2, lg: 6 }} ml={{ base: 2, lg: 'auto' }} _empty={{ display: 'none' }}>
            <AddressAdvancedFilterLink
              isLoading={ localQuery.query.isPlaceholderData }
              address={ hash }
              typeFilter={ localQuery.filters.type }
              directionFilter={ localQuery.filters.filter }
            />
            <AddressCsvExportLink
              address={ hash }
              params={{ type: 'token-transfers', filterType: 'address', filterValue: localQuery.filters.filter }}
              isLoading={ localQuery.query.isPlaceholderData }
            />
          </HStack>
          <Pagination ml={{ base: 'auto', lg: 8 }} { ...localQuery.query.pagination }/>
        </>
      );
    }

    if (config.features.crossChainTxs.isEnabled) {
      if (isMobile) {
        return null;
      }
      return <Pagination ml="auto" { ...crossChainQuery.pagination }/>;
    }

    return null;
  })();

  const rightSlotProps = (() => {
    return {
      display: 'flex',
      justifyContent: { base: 'flex-end', lg: 'space-between' },
      ml: tabs.length > 1 ? { base: 0, lg: 4 } : 0,
      widthAllocation: 'available' as const,
    };
  })();

  return (
    <RoutedTabs
      variant="secondary"
      size="sm"
      tabs={ tabs }
      onValueChange={ handleTabValueChange }
      rightSlot={ rightSlot }
      rightSlotProps={ rightSlotProps }
      listProps={ isMobile ? undefined : TAB_LIST_PROPS }
      stickyEnabled={ !isMobile }
    />
  );
};

export default React.memo(AddressTokenTransfers);
