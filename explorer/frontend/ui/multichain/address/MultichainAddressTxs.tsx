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
import AddressCsvExportLink from 'ui/address/AddressCsvExportLink';
import AddressTxsFilter from 'ui/address/AddressTxsFilter';
import useAddressTxsQuery from 'ui/address/useAddressTxsQuery';
import useAddressCountersQuery from 'ui/address/utils/useAddressCountersQuery';
import ChainSelect from 'ui/multichain/components/ChainSelect';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import Pagination from 'ui/shared/pagination/Pagination';
import TxsWithAPISorting from 'ui/txs/TxsWithAPISorting';

import ListCounterText from '../components/ListCounterText';
import getAvailableChainIds from './getAvailableChainIds';

export const ADDRESS_MULTICHAIN_TXS_TAB_IDS = [ 'txs_cross_chain' as const, 'txs_local' as const ];
const TAB_LIST_PROPS = {
  marginBottom: 0,
  pt: 6,
  pb: 3,
  marginTop: -6,
};
const TABS_RIGHT_SLOT_PROPS = {
  display: 'flex',
  justifyContent: { base: 'flex-end', lg: 'space-between' },
  ml: { base: 0, lg: 8 },
  widthAllocation: 'available' as const,
};

interface Props {
  addressData: multichain.GetAddressResponse | undefined;
  isLoading: boolean;
}

const MultichainAddressTxs = ({ addressData, isLoading }: Props) => {
  const router = useRouter();
  const isMobile = useIsMobile();

  const hash = getQueryParamString(router.query.hash);
  const tab = getQueryParamString(router.query.tab) as typeof ADDRESS_MULTICHAIN_TXS_TAB_IDS[number] | 'txs' | undefined;
  const isLocalTab = tab === 'txs_local' || tab === 'txs';

  const chainIds = React.useMemo(() => getAvailableChainIds(addressData), [ addressData ]);

  const txsQueryLocal = useAddressTxsQuery({
    addressHash: hash,
    enabled: !isLoading && isLocalTab && chainIds.length > 0,
    isMultichain: true,
    chainIds,
  });

  const chainId = txsQueryLocal.query.chainValue?.[0];
  const chainData = multichainConfig()?.chains.find(chain => chain.id === chainId);

  const countersQueryLocal = useAddressCountersQuery({
    hash,
    isLoading: txsQueryLocal.query.isPlaceholderData || isLoading,
    isEnabled: !isLoading && isLocalTab && chainIds.length > 0,
    chain: chainData,
  });

  const txsLocalFilter = isLocalTab ? (
    <AddressTxsFilter
      initialValue={ txsQueryLocal.filterValue }
      onFilterChange={ txsQueryLocal.onFilterChange }
      hasActiveFilter={ Boolean(txsQueryLocal.filterValue) }
      isLoading={ txsQueryLocal.query.pagination.isLoading }
    />
  ) : null;

  const countersText = (() => {
    if (isLocalTab) {
      return (
        <ListCounterText
          key={ chainId }
          value={ countersQueryLocal.data?.transactions_count }
          isLoading={ countersQueryLocal.isPlaceholderData || txsQueryLocal.query.isPlaceholderData }
          type="transaction"
        />
      );
    }

    return null;
  })();

  const chainSelect = (
    <ChainSelect
      loading={ txsQueryLocal.query.pagination.isLoading }
      value={ txsQueryLocal.query.chainValue }
      onValueChange={ txsQueryLocal.query.onChainValueChange }
      chainIds={ chainIds }
    />
  );

  const rightSlot = (() => {
    if (isLocalTab) {
      if (chainIds.length === 0) {
        return null;
      }

      if (isMobile) {
        return chainSelect;
      }

      return (
        <>
          <HStack gap={ 2 }>
            { txsLocalFilter }
            { chainSelect }
            { countersText }
          </HStack>
          <HStack gap={ 6 }>
            <AddressCsvExportLink
              address={ hash }
              params={{ type: 'transactions', filterType: 'address', filterValue: txsQueryLocal.filterValue }}
              isLoading={ txsQueryLocal.query.pagination.isLoading }
              chainData={ chainData }
            />
            <Pagination { ...txsQueryLocal.query.pagination }/>
          </HStack>
        </>
      );
    }

    return null;
  })();

  const tabs: Array<TabItemRegular> = [
    {
      id: 'txs_cross_chain',
      title: 'Cross-chain',
      component: <EmptyState type="coming_soon"/>,
    },
    {
      id: [ 'txs_local', 'txs' ],
      title: 'Local',
      component: chainIds.length > 0 ? (
        <SocketProvider url={ getSocketUrl(chainData?.app_config) }>
          <MultichainProvider chainId={ chainId }>
            { isMobile && countersText }
            <TxsWithAPISorting
              filter={ txsLocalFilter }
              filterValue={ txsQueryLocal.filterValue }
              query={ txsQueryLocal.query }
              currentAddress={ hash }
              enableTimeIncrement
              socketType="address_txs"
              top={ ACTION_BAR_HEIGHT_DESKTOP }
              sorting={ txsQueryLocal.sort }
              setSort={ txsQueryLocal.setSort }
            />
          </MultichainProvider>
        </SocketProvider>
      ) : <p>There are no transactions.</p>,
    },
  ];

  return (
    <RoutedTabs
      variant="secondary"
      size="sm"
      tabs={ tabs }
      defaultTabId="txs_local"
      rightSlot={ rightSlot }
      rightSlotProps={ TABS_RIGHT_SLOT_PROPS }
      listProps={ isMobile ? undefined : TAB_LIST_PROPS }
      stickyEnabled={ !isMobile }
    />
  );
};

export default React.memo(MultichainAddressTxs);
