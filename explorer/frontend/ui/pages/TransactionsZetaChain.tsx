import { useRouter } from 'next/router';
import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import getQueryParamString from 'lib/router/getQueryParamString';
import { SocketProvider } from 'lib/socket/context';
import { TX } from 'stubs/tx';
import { generateListStub } from 'stubs/utils';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import ActionBar from 'ui/shared/ActionBar';
import AdvancedFilterLink from 'ui/shared/links/AdvancedFilterLink';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import useIsAuth from 'ui/snippets/auth/useIsAuth';
import TxsStats from 'ui/txs/TxsStats';
import TxsWatchlist from 'ui/txs/TxsWatchlist';
import TxsWithFrontendSorting from 'ui/txs/TxsWithFrontendSorting';
import ZetaChainCCTXsTab from 'ui/zetaChain/ZetaChainCCTXsTab';
import ZetaChainEvmTransactions from 'ui/zetaChain/ZetaChainEvmTransactions';

const ZETACHAIN_TABS = [ 'zetachain_validated', 'zetachain_pending' ];
const CROSS_CHAIN_TABS = [ 'cctx_pending', 'cctx_mined' ];

const TransactionsZetaChain = () => {
  const router = useRouter();
  const isMobile = useIsMobile();
  const tab = getQueryParamString(router.query.tab);

  const txsWithBlobsQuery = useQueryWithPages({
    resourceName: 'general:txs_with_blobs',
    filters: { type: 'blob_transaction' },
    options: {
      enabled: config.features.dataAvailability.isEnabled && tab === 'blob_txs',
      placeholderData: generateListStub<'general:txs_with_blobs'>(TX, 50, { next_page_params: {
        block_number: 10602877,
        index: 8,
        items_count: 50,
      } }),
    },
  });

  const txsWatchlistQuery = useQueryWithPages({
    resourceName: 'general:txs_watchlist',
    options: {
      enabled: tab === 'watchlist',
      placeholderData: generateListStub<'general:txs_watchlist'>(TX, 50, { next_page_params: {
        block_number: 9005713,
        index: 5,
        items_count: 50,
      } }),
    },
  });

  const isAuth = useIsAuth();

  // for cctxs and evm txs we show pagination with the secondary tabs
  const pagination = (() => {
    switch (tab) {
      case 'watchlist': return txsWatchlistQuery.pagination;
      case 'blob_txs': return txsWithBlobsQuery.pagination;
      default: return null;
    }
  })();

  const topRow = (() => {
    if (isMobile) {
      return null;
    }

    if (tab !== 'blob_txs' && tab !== 'watchlist') {
      return null;
    }

    const isAdvancedFilterEnabled = config.features.advancedFilter.isEnabled;

    if (!isAdvancedFilterEnabled && !pagination?.isVisible) {
      return null;
    }

    return (
      <ActionBar
        mt={ -6 }
        display={{ base: pagination?.isVisible ? 'flex' : 'none', lg: 'flex' }}
        justifyContent="end"
        gap={ 6 }
      >
        { isAdvancedFilterEnabled && <AdvancedFilterLink/> }
        { pagination?.isVisible && <Pagination { ...pagination }/> }
      </ActionBar>
    );
  })();

  const tabs: Array<TabItemRegular> = [
    {
      id: 'cctx',
      title: 'Cross chain',
      component: (
        <SocketProvider url={ config.apis.zetachain?.socketEndpoint } name="zetachain">
          <ZetaChainCCTXsTab/>
        </SocketProvider>
      ),
      subTabs: CROSS_CHAIN_TABS,
    },
    {
      id: 'zetachain',
      title: 'ZetaChain EVM',
      component: <ZetaChainEvmTransactions/>,
      subTabs: ZETACHAIN_TABS,
    },
    config.features.dataAvailability.isEnabled && {
      id: 'blob_txs',
      title: 'Blob txns',
      component: (
        <>
          <TxsStats/>
          { topRow }
          <TxsWithFrontendSorting query={ txsWithBlobsQuery }/>
        </>
      ),
    },
    isAuth ? {
      id: 'watchlist',
      title: 'Watch list',
      component: (
        <>
          <TxsStats/>
          { topRow }
          <TxsWatchlist query={ txsWatchlistQuery }/>
        </>
      ),
    } : undefined,
  ].filter(Boolean);

  return (
    <>
      <PageTitle
        title={ config.meta.seo.enhancedDataEnabled ? `${ config.chain.name } transactions` : 'Transactions' }
        withTextAd
      />
      <RoutedTabs tabs={ tabs } defaultTabId="zetachain"/>
    </>
  );
};

export default TransactionsZetaChain;
