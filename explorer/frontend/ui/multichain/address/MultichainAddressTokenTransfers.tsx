import { HStack } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import multichainConfig from 'configs/multichain';
import getSocketUrl from 'lib/api/getSocketUrl';
import { MultichainProvider } from 'lib/contexts/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import getQueryParamString from 'lib/router/getQueryParamString';
import { SocketProvider } from 'lib/socket/context';
import { EmptyState } from 'toolkit/chakra/empty-state';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import AddressAdvancedFilterLink from 'ui/address/AddressAdvancedFilterLink';
import AddressCsvExportLink from 'ui/address/AddressCsvExportLink';
import AddressTokenTransfersLocal from 'ui/address/AddressTokenTransfersLocal';
import useAddressTokenTransfersQuery from 'ui/address/useAddressTokenTransfersQuery';
import useAddressCountersQuery from 'ui/address/utils/useAddressCountersQuery';
import ChainSelect from 'ui/multichain/components/ChainSelect';
import Pagination from 'ui/shared/pagination/Pagination';
import TokenTransferFilter from 'ui/shared/TokenTransfer/TokenTransferFilter';
import { getTokenFilterValue } from 'ui/tokens/utils';

import ListCounterText from '../components/ListCounterText';
import getAvailableChainIds from './getAvailableChainIds';

export const ADDRESS_MULTICHAIN_TOKEN_TRANSFERS_TAB_IDS = [ 'token_transfers_cross_chain' as const, 'token_transfers_local' as const ];
const TABS_RIGHT_SLOT_PROPS = {
  display: 'flex',
  justifyContent: { base: 'flex-end', lg: 'space-between' },
  ml: { base: 0, lg: 8 },
  widthAllocation: 'available' as const,
};
const TAB_LIST_PROPS = {
  marginBottom: 0,
  pt: 6,
  pb: 3,
  marginTop: -6,
};

interface Props {
  addressData: multichain.GetAddressResponse | undefined;
  isLoading: boolean;
}

const MultichainAddressTokenTransfers = ({ addressData, isLoading }: Props) => {
  const router = useRouter();
  const isMobile = useIsMobile();

  const hash = getQueryParamString(router.query.hash);
  const tab = getQueryParamString(router.query.tab) as typeof ADDRESS_MULTICHAIN_TOKEN_TRANSFERS_TAB_IDS[number] | 'token_transfers' | undefined;
  const isLocalTab = tab === 'token_transfers_local' || tab === 'token_transfers';

  const chainIds = React.useMemo(() => getAvailableChainIds(addressData), [ addressData ]);

  const transfersQueryLocal = useAddressTokenTransfersQuery({
    currentAddress: hash,
    enabled: isLocalTab && !isLoading && chainIds.length > 0,
    isMultichain: true,
    chainIds,
  });

  const chainId = transfersQueryLocal.query.chainValue?.[0];
  const chainData = multichainConfig()?.chains.find(chain => chain.id === chainId);

  const countersQueryLocal = useAddressCountersQuery({
    hash,
    isLoading: transfersQueryLocal.query.isPlaceholderData,
    isEnabled: isLocalTab && !isLoading && chainIds.length > 0,
    chain: chainData,
  });

  const handleChainValueChange = React.useCallback(({ value }: { value: Array<string> }) => {
    transfersQueryLocal.query.onChainValueChange({ value });
    const chainConfig = multichainConfig()?.chains.find(chain => chain.id === value[0]);
    const tokenTypes = getTokenFilterValue(router.query.type, chainConfig?.app_config);
    if (tokenTypes) {
      const chainsTokenTypes = transfersQueryLocal.filters.type.filter(type => tokenTypes.includes(type));
      if (chainsTokenTypes.length < transfersQueryLocal.filters.type.length) {
        transfersQueryLocal.onTypeFilterChange(chainsTokenTypes);
      }
    }
  }, [ transfersQueryLocal, router.query.type ]);

  const countersText = (() => {
    if (isLocalTab) {
      return (
        <ListCounterText
          key={ chainId }
          value={ countersQueryLocal.data?.token_transfers_count }
          isLoading={ countersQueryLocal.isPlaceholderData || transfersQueryLocal.query.isPlaceholderData }
          type="transfer"
        />
      );
    }

    return null;
  })();

  const rightSlot = (() => {
    if (isLocalTab) {
      if (chainIds.length === 0) {
        return null;
      }

      const chainSelect = (
        <ChainSelect
          loading={ transfersQueryLocal.query.pagination.isLoading }
          value={ transfersQueryLocal.query.chainValue }
          onValueChange={ handleChainValueChange }
          chainIds={ chainIds }
        />
      );

      if (isMobile) {
        return chainSelect;
      }

      return (
        <>
          <HStack gap={ 2 }>
            <TokenTransferFilter
              defaultTypeFilters={ transfersQueryLocal.filters.type }
              onTypeFilterChange={ transfersQueryLocal.onTypeFilterChange }
              appliedFiltersNum={ transfersQueryLocal.filters.type.length + (transfersQueryLocal.filters.filter ? 1 : 0) }
              withAddressFilter
              onAddressFilterChange={ transfersQueryLocal.onAddressFilterChange }
              defaultAddressFilter={ transfersQueryLocal.filters.filter }
              isLoading={ transfersQueryLocal.query.isPlaceholderData }
              chainConfig={ chainData?.app_config }
            />
            { chainSelect }
            { countersText }
          </HStack>
          <HStack gap={ 6 }>
            <AddressAdvancedFilterLink
              isLoading={ transfersQueryLocal.query.isPlaceholderData }
              address={ hash }
              typeFilter={ transfersQueryLocal.filters.type }
              directionFilter={ transfersQueryLocal.filters.filter }
              chainData={ chainData }
            />
            <AddressCsvExportLink
              address={ hash }
              params={{ type: 'token-transfers', filterType: 'address', filterValue: transfersQueryLocal.filters.filter }}
              isLoading={ transfersQueryLocal.query.pagination.isLoading }
              chainData={ chainData }
            />
            <Pagination { ...transfersQueryLocal.query.pagination }/>
          </HStack>
        </>
      );
    }

    return null;
  })();

  const tabs: Array<TabItemRegular> = [
    {
      id: 'token_transfers_cross_chain',
      title: 'Cross-chain',
      component: <EmptyState type="coming_soon"/>,
    },
    {
      id: [ 'token_transfers_local', 'token_transfers' ],
      title: 'Local',
      component: chainIds.length > 0 ? (
        <MultichainProvider chainId={ transfersQueryLocal.query.chainValue?.[0] }>
          <SocketProvider url={ getSocketUrl(chainData?.app_config) }>
            { isMobile && countersText }
            <AddressTokenTransfersLocal
              query={ transfersQueryLocal.query }
              filters={ transfersQueryLocal.filters }
              onTypeFilterChange={ transfersQueryLocal.onTypeFilterChange }
              onAddressFilterChange={ transfersQueryLocal.onAddressFilterChange }
              addressHash={ hash }
            />
          </SocketProvider>
        </MultichainProvider>
      ) : <p>There are no token transfers.</p>,
    },
  ];

  return (
    <RoutedTabs
      variant="secondary"
      size="sm"
      tabs={ tabs }
      isLoading={ isLoading }
      defaultTabId="token_transfers_local"
      rightSlot={ rightSlot }
      rightSlotProps={ TABS_RIGHT_SLOT_PROPS }
      listProps={ isMobile ? undefined : TAB_LIST_PROPS }
      stickyEnabled={ !isMobile }
    />
  );
};

export default React.memo(MultichainAddressTokenTransfers);
