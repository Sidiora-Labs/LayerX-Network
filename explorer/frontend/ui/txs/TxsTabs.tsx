import { Flex } from '@chakra-ui/react';
import { capitalize } from 'es-toolkit';
import { useRouter } from 'next/router';
import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import useIsInitialLoading from 'lib/hooks/useIsInitialLoading';
import useIsMobile from 'lib/hooks/useIsMobile';
import getNetworkValidationActionText from 'lib/networks/getNetworkValidationActionText';
import getQueryParamString from 'lib/router/getQueryParamString';
import { TX } from 'stubs/tx';
import { generateListStub } from 'stubs/utils';
import type { RoutedTabsProps } from 'toolkit/components/RoutedTabs/RoutedTabs';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import AdvancedFilterLink from 'ui/shared/links/AdvancedFilterLink';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import useIsAuth from 'ui/snippets/auth/useIsAuth';
import TxsWithFrontendSorting from 'ui/txs/TxsWithFrontendSorting';

import TxsWatchlist from './TxsWatchlist';

type TabId = 'validated' | 'pending' | 'blob_txs' | 'watchlist';
export const getTabId = (id: TabId, prefix?: string) => prefix ? `${ prefix }_${ id }` : id;

interface Props extends Omit<RoutedTabsProps, 'tabs'> {
  parentTab?: string;
  tabsHeight?: number;
}

const TxsTabs = ({ parentTab, tabsHeight, ...rest }: Props) => {
  const isMobile = useIsMobile();
  const router = useRouter();
  const tab = getQueryParamString(router.query.tab);
  const multichainContext = useMultichainContext();

  const chainConfig = multichainContext?.chain.app_config ?? config;

  const txsValidatedQuery = useQueryWithPages({
    resourceName: 'general:txs_validated',
    filters: { filter: 'validated' },
    options: {
      enabled: tab === getTabId('validated', parentTab) ||
        (parentTab ? tab === parentTab : true) ||
        !tab ||
        (!chainConfig?.features.dataAvailability.isEnabled && tab === getTabId('blob_txs', parentTab)),
      placeholderData: generateListStub<'general:txs_validated'>(TX, 50, { next_page_params: {
        block_number: 9005713,
        index: 5,
        items_count: 50,
        filter: 'validated',
      } }),
    },
  });

  const txsPendingQuery = useQueryWithPages({
    resourceName: 'general:txs_pending',
    filters: { filter: 'pending' },
    options: {
      enabled: tab === getTabId('pending', parentTab),
      placeholderData: generateListStub<'general:txs_pending'>(TX, 50, { next_page_params: {
        inserted_at: '2024-02-05T07:04:47.749818Z',
        hash: '0x00',
        filter: 'pending',
      } }),
    },
  });

  const txsWithBlobsQuery = useQueryWithPages({
    resourceName: 'general:txs_with_blobs',
    filters: { type: 'blob_transaction' },
    options: {
      enabled: chainConfig?.features.dataAvailability.isEnabled && tab === getTabId('blob_txs', parentTab),
      placeholderData: generateListStub<'general:txs_with_blobs'>(TX, 50, { next_page_params: {
        block_number: 10602877,
        index: 8,
        items_count: 50,
      } }),
    },
  });

  const isAuth = useIsAuth();

  const txsWatchlistQuery = useQueryWithPages({
    resourceName: 'general:txs_watchlist',
    options: {
      enabled: isAuth && tab === getTabId('watchlist', parentTab),
      placeholderData: generateListStub<'general:txs_watchlist'>(TX, 50, { next_page_params: {
        block_number: 9005713,
        index: 5,
        items_count: 50,
      } }),
    },
  });

  const verifiedTitle = capitalize(getNetworkValidationActionText(chainConfig));

  const tabs: Array<TabItemRegular> = [
    {
      id: getTabId('validated', parentTab),
      title: verifiedTitle,
      component:
        <TxsWithFrontendSorting
          query={ txsValidatedQuery }
          socketType="txs_validated"
          top={ tabsHeight }
        /> },
    {
      id: getTabId('pending', parentTab),
      title: 'Pending',
      component: (
        <TxsWithFrontendSorting
          query={ txsPendingQuery }
          showBlockInfo={ false }
          socketType="txs_pending"
          top={ tabsHeight }
        />
      ),
    },
    chainConfig?.features.dataAvailability.isEnabled && {
      id: getTabId('blob_txs', parentTab),
      title: 'Blob txns',
      component: (
        <TxsWithFrontendSorting
          query={ txsWithBlobsQuery }
          top={ tabsHeight }
        />
      ),
    },
    isAuth && {
      id: getTabId('watchlist', parentTab),
      title: 'Watch list',
      component: <TxsWatchlist query={ txsWatchlistQuery } top={ tabsHeight }/>,
    },
  ].filter(Boolean);

  const currentQuery = (() => {
    switch (tab) {
      case getTabId('pending', parentTab): {
        return txsPendingQuery;
      };
      case getTabId('watchlist', parentTab): {
        return txsWatchlistQuery;
      };
      case getTabId('blob_txs', parentTab): {
        if (chainConfig?.features.dataAvailability.isEnabled) {
          return txsWithBlobsQuery;
        }
        return txsValidatedQuery;
      };
      default: return txsValidatedQuery;
    }
  })();

  const isTabsLoading = useIsInitialLoading(currentQuery.isPlaceholderData);

  const rightSlot = (() => {
    if (isMobile) {
      return null;
    }

    const isAdvancedFilterEnabled = chainConfig?.features.advancedFilter.isEnabled;

    if (!isAdvancedFilterEnabled && !currentQuery.pagination.isVisible) {
      return null;
    }

    return (
      <Flex alignItems="center" gap={ 6 }>
        { isAdvancedFilterEnabled && <AdvancedFilterLink routeParams={{ chain: multichainContext?.chain }}/> }
        { currentQuery.pagination.isVisible && <Pagination { ...currentQuery.pagination }/> }
      </Flex>
    );
  })();

  return (
    <RoutedTabs
      tabs={ tabs }
      isLoading={ isTabsLoading }
      stickyEnabled={ !isMobile }
      rightSlot={ rightSlot }
      { ...rest }
    />
  );
};

export default React.memo(TxsTabs);
