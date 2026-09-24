import { useRouter } from 'next/router';
import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import throwOnAbsentParamError from 'lib/errors/throwOnAbsentParamError';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import useIsMobile from 'lib/hooks/useIsMobile';
import getQueryParamString from 'lib/router/getQueryParamString';
import { BLOCK } from 'stubs/block';
import { TX } from 'stubs/tx';
import { generateListStub } from 'stubs/utils';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import BlocksContent from 'ui/blocks/BlocksContent';
import TextAd from 'ui/shared/ad/TextAd';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import OptimisticL2TxnBatchDetails from 'ui/txnBatches/optimisticL2/OptimisticL2TxnBatchDetails';
import useBatchQuery from 'ui/txnBatches/optimisticL2/useBatchQuery';
import TxsWithFrontendSorting from 'ui/txs/TxsWithFrontendSorting';

const TAB_LIST_PROPS = {
  marginBottom: 0,
  py: 5,
  marginTop: -5,
};

const TABS_HEIGHT = 80;

const OptimisticL2TxnBatch = () => {
  const router = useRouter();
  const number = getQueryParamString(router.query.number);
  const height = getQueryParamString(router.query.height);
  const commitment = getQueryParamString(router.query.commitment);
  const tab = getQueryParamString(router.query.tab);
  const isMobile = useIsMobile();

  const batchQuery = useBatchQuery();

  const batchTxsQuery = useQueryWithPages({
    resourceName: 'general:optimistic_l2_txn_batch_txs',
    pathParams: { number: String(batchQuery.data?.number) },
    options: {
      enabled: Boolean(!batchQuery.isPlaceholderData && batchQuery.data?.number && tab === 'txs'),
      placeholderData: generateListStub<'general:optimistic_l2_txn_batch_txs'>(TX, 50, { next_page_params: {
        block_number: 1338932,
        index: 1,
        items_count: 50,
      } }),
    },
  });

  const batchBlocksQuery = useQueryWithPages({
    resourceName: 'general:optimistic_l2_txn_batch_blocks',
    pathParams: { number: String(batchQuery.data?.number) },
    options: {
      enabled: Boolean(!batchQuery.isPlaceholderData && batchQuery.data?.number && tab === 'blocks'),
      placeholderData: generateListStub<'general:optimistic_l2_txn_batch_blocks'>(BLOCK, 50, { next_page_params: {
        batch_number: 1338932,
        items_count: 50,
      } }),
    },
  });

  throwOnAbsentParamError(number || (height && commitment));
  throwOnResourceLoadError(batchQuery);

  let pagination;
  if (tab === 'txs') {
    pagination = batchTxsQuery.pagination;
  }
  if (tab === 'blocks') {
    pagination = batchBlocksQuery.pagination;
  }

  const hasPagination = !isMobile && pagination?.isVisible;

  const tabs: Array<TabItemRegular> = React.useMemo(() => ([
    { id: 'index', title: 'Details', component: <OptimisticL2TxnBatchDetails query={ batchQuery }/> },
    {
      id: 'txs',
      title: 'Transactions',
      component: <TxsWithFrontendSorting query={ batchTxsQuery } top={ hasPagination ? TABS_HEIGHT : 0 }/>,
    },
    {
      id: 'blocks',
      title: 'Blocks',
      component: <BlocksContent type="block" query={ batchBlocksQuery } enableSocket={ false } top={ hasPagination ? TABS_HEIGHT : 0 }/>,
    },
  ].filter(Boolean)), [ batchQuery, batchTxsQuery, batchBlocksQuery, hasPagination ]);

  return (
    <>
      <TextAd mb={ 6 }/>
      <PageTitle
        title={ `Batch #${ batchQuery.data?.number }` }
        isLoading={ batchQuery.isPlaceholderData }
      />
      <RoutedTabs
        tabs={ tabs }
        isLoading={ batchQuery.isPlaceholderData }
        listProps={ isMobile ? undefined : TAB_LIST_PROPS }
        rightSlot={ hasPagination && pagination ? <Pagination { ...(pagination) }/> : null }
        stickyEnabled={ hasPagination }
      />
    </>
  );
};

export default OptimisticL2TxnBatch;
