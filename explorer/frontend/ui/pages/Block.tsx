import { chakra, Flex } from '@chakra-ui/react';
import { capitalize } from 'es-toolkit';
import { useRouter } from 'next/router';
import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';
import type { PaginationParams } from 'ui/shared/pagination/types';

import { routeParams } from 'nextjs/routes';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import throwOnAbsentParamError from 'lib/errors/throwOnAbsentParamError';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import useIsMobile from 'lib/hooks/useIsMobile';
import getNetworkValidatorTitle from 'lib/networks/getNetworkValidatorTitle';
import getQueryParamString from 'lib/router/getQueryParamString';
import { Skeleton } from 'toolkit/chakra/skeleton';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import BlockCeloEpochTag from 'ui/block/BlockCeloEpochTag';
import BlockDeposits from 'ui/block/BlockDeposits';
import BlockDetails from 'ui/block/BlockDetails';
import BlockInternalTxs from 'ui/block/BlockInternalTxs';
import BlockWithdrawals from 'ui/block/BlockWithdrawals';
import useBlockBlobTxsQuery from 'ui/block/useBlockBlobTxsQuery';
import useBlockDepositsQuery from 'ui/block/useBlockDepositsQuery';
import useBlockInternalTxsQuery from 'ui/block/useBlockInternalTxsQuery';
import useBlockQuery from 'ui/block/useBlockQuery';
import useBlockTxsQuery from 'ui/block/useBlockTxsQuery';
import useBlockWithdrawalsQuery from 'ui/block/useBlockWithdrawalsQuery';
import TextAd from 'ui/shared/ad/TextAd';
import ServiceDegradationWarning from 'ui/shared/alerts/ServiceDegradationWarning';
import BlockPendingUpdateAlert from 'ui/shared/block/BlockPendingUpdateAlert';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import * as BlockEntity from 'ui/shared/entities/block/BlockEntity';
import NetworkExplorers from 'ui/shared/NetworkExplorers';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import TxsWithFrontendSorting from 'ui/txs/TxsWithFrontendSorting';

const TAB_LIST_PROPS = {
  marginBottom: 0,
  pt: 6,
  pb: 6,
  marginTop: -5,
};
const TABS_HEIGHT = 88;

const beaconChainFeature = config.features.beaconChain;

const BlockPageContent = () => {
  const router = useRouter();
  const isMobile = useIsMobile();
  const heightOrHash = getQueryParamString(router.query.height_or_hash);
  const tab = getQueryParamString(router.query.tab);
  const multichainContext = useMultichainContext();

  const blockQuery = useBlockQuery({ heightOrHash });
  const blockTxsQuery = useBlockTxsQuery({ heightOrHash, blockQuery, tab });
  const blockWithdrawalsQuery = useBlockWithdrawalsQuery({ heightOrHash, blockQuery, tab });
  const blockDepositsQuery = useBlockDepositsQuery({ heightOrHash, blockQuery, tab });
  const blockBlobTxsQuery = useBlockBlobTxsQuery({ heightOrHash, blockQuery, tab });
  const blockInternalTxsQuery = useBlockInternalTxsQuery({ heightOrHash, blockQuery, tab });

  const hasPagination = !isMobile && (
    (tab === 'txs' && blockTxsQuery.pagination.isVisible) ||
    (tab === 'withdrawals' && blockWithdrawalsQuery.pagination.isVisible) ||
    (tab === 'deposits' && blockDepositsQuery.pagination.isVisible) ||
    (tab === 'internal_txs' && blockInternalTxsQuery.pagination.isVisible)
  );

  const tabs: Array<TabItemRegular> = React.useMemo(() => ([
    {
      id: 'index',
      title: 'Details',
      component: (
        <>
          <Flex rowGap={{ base: 1, lg: 2 }} mb={{ base: 3, lg: 6 }} flexDir="column">
            { blockQuery.isDegradedData && <ServiceDegradationWarning isLoading={ blockQuery.isPlaceholderData }/> }
            { blockQuery.data?.is_pending_update && <BlockPendingUpdateAlert/> }
          </Flex>
          <BlockDetails query={ blockQuery }/>
        </>
      ),
    },
    {
      id: 'txs',
      title: 'Transactions',
      component: (
        <>
          { blockTxsQuery.isDegradedData && <ServiceDegradationWarning isLoading={ blockTxsQuery.isPlaceholderData } mb={{ base: 3, lg: 6 }}/> }
          <TxsWithFrontendSorting query={ blockTxsQuery } showBlockInfo={ false } top={ hasPagination ? TABS_HEIGHT : 0 }/>
        </>
      ),
    },
    {
      id: 'internal_txs',
      title: 'Internal txns',
      component: (
        <>
          { blockTxsQuery.isDegradedData && <ServiceDegradationWarning isLoading={ blockTxsQuery.isPlaceholderData } mb={{ base: 3, lg: 6 }}/> }
          <BlockInternalTxs query={ blockInternalTxsQuery } top={ hasPagination ? TABS_HEIGHT : 0 }/>
        </>
      ),
    },
    config.features.dataAvailability.isEnabled && blockQuery.data?.blob_transactions_count ?
      {
        id: 'blob_txs',
        title: 'Blob txns',
        component: (
          <TxsWithFrontendSorting query={ blockBlobTxsQuery } showBlockInfo={ false }/>
        ),
      } : null,
    beaconChainFeature.isEnabled && !beaconChainFeature.withdrawalsOnly && Boolean(blockQuery.data?.beacon_deposits_count) ?
      {
        id: 'deposits',
        title: 'Deposits',
        component: (
          <>
            { blockDepositsQuery.isDegradedData && <ServiceDegradationWarning isLoading={ blockDepositsQuery.isPlaceholderData } mb={{ base: 3, lg: 6 }}/> }
            <BlockDeposits blockDepositsQuery={ blockDepositsQuery }/>
          </>
        ),
      } : null,
    config.features.beaconChain.isEnabled && Boolean(blockQuery.data?.withdrawals_count) ?
      {
        id: 'withdrawals',
        title: 'Withdrawals',
        component: (
          <>
            { blockWithdrawalsQuery.isDegradedData &&
              <ServiceDegradationWarning isLoading={ blockWithdrawalsQuery.isPlaceholderData } mb={{ base: 3, lg: 6 }}/> }
            <BlockWithdrawals blockWithdrawalsQuery={ blockWithdrawalsQuery }/>
          </>
        ),
      } : null,
  ].filter(Boolean)), [ blockBlobTxsQuery, blockDepositsQuery, blockInternalTxsQuery, blockQuery, blockTxsQuery, blockWithdrawalsQuery, hasPagination ]);

  let pagination;
  if (tab === 'txs') {
    pagination = blockTxsQuery.pagination;
  } else if (tab === 'withdrawals') {
    pagination = blockWithdrawalsQuery.pagination;
  } else if (tab === 'deposits') {
    pagination = blockDepositsQuery.pagination;
  } else if (tab === 'internal_txs') {
    pagination = blockInternalTxsQuery.pagination;
  }

  throwOnAbsentParamError(heightOrHash);

  if (blockQuery.isError) {
    if (!blockQuery.isDegradedData && blockQuery.error.status === 404 && !heightOrHash.startsWith('0x') && blockQuery.isFutureBlock) {
      const url = routeParams({ pathname: '/block/countdown/[height]', query: { height: heightOrHash } }, multichainContext);
      router.push(url, undefined, { shallow: true });
      return null;
    } else {
      throwOnResourceLoadError(blockQuery);
    }
  }

  const title = (() => {
    switch (blockQuery.data?.type) {
      case 'reorg':
        return `Reorged block #${ blockQuery.data?.height }`;

      case 'uncle':
        return `Uncle block #${ blockQuery.data?.height }`;

      default:
        return `Block #${ blockQuery.data?.height }`;
    }
  })();

  const beforeTitleElement = multichainContext?.chain ? (
    <BlockEntity.Icon variant="heading" chain={ multichainContext.chain } isLoading={ blockQuery.isPlaceholderData }/>
  ) : null;

  const titleSecondRow = (
    <>
      { !config.UI.views.block.hiddenFields?.miner && blockQuery.data?.miner && (
        <Skeleton
          loading={ blockQuery.isPlaceholderData }
          fontFamily="heading"
          display="flex"
          minW={ 0 }
          columnGap={ 2 }
          fontWeight={ 500 }
        >
          <chakra.span flexShrink={ 0 }>
            { capitalize(getNetworkValidatorTitle()) }
          </chakra.span>
          <AddressEntity address={ blockQuery.data.miner }/>
        </Skeleton>
      ) }
      <NetworkExplorers
        type="block"
        pathParam={ heightOrHash }
        ml={{ base: config.UI.views.block.hiddenFields?.miner ? 0 : 3, lg: 'auto' }}
      />
    </>
  );

  return (
    <>
      <TextAd mb={ 6 }/>
      <PageTitle
        title={ title }
        beforeTitle={ beforeTitleElement }
        contentAfter={ <BlockCeloEpochTag blockQuery={ blockQuery }/> }
        secondRow={ titleSecondRow }
        isLoading={ blockQuery.isPlaceholderData }
      />
      <RoutedTabs
        tabs={ tabs }
        isLoading={ blockQuery.isPlaceholderData }
        listProps={ isMobile ? undefined : TAB_LIST_PROPS }
        rightSlot={ hasPagination ? <Pagination { ...(pagination as PaginationParams) }/> : null }
        stickyEnabled={ hasPagination }
      />
    </>
  );
};

export default BlockPageContent;
