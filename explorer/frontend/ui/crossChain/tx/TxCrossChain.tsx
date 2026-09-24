import { useRouter } from 'next/router';
import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import useApiQuery from 'lib/api/useApiQuery';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import getQueryParamString from 'lib/router/getQueryParamString';
import { INTERCHAIN_MESSAGE } from 'stubs/interchainIndexer';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import TextAd from 'ui/shared/ad/TextAd';
import CrossChainMessageEntity from 'ui/shared/entities/crossChainMessage/CrossChainMessageEntity';
import PageTitle from 'ui/shared/Page/PageTitle';

import TxCrossChainDetails from './TxCrossChainDetails';
import TxCrossChainTransfers from './TxCrossChainTransfers';

const TxCrossChain = () => {
  const router = useRouter();
  const messageId = getQueryParamString(router.query.id);

  const query = useApiQuery('interchainIndexer:message', {
    pathParams: {
      id: messageId,
    },
    queryOptions: {
      placeholderData: INTERCHAIN_MESSAGE,
    },
  });

  const tabs: Array<TabItemRegular> = [
    {
      id: 'index',
      title: 'Details',
      component: <TxCrossChainDetails data={ query.data } isLoading={ query.isPlaceholderData }/>,
    },
    {
      id: 'transfers',
      title: 'Token transfers',
      component: <TxCrossChainTransfers data={ query.data?.transfers } isLoading={ query.isPlaceholderData } isError={ query.isError }/>,
    },
  ];

  throwOnResourceLoadError(query);

  const titleSecondRow = (
    <CrossChainMessageEntity
      id={ messageId }
      isLoading={ query.isPlaceholderData }
      variant="subheading"
      truncation="dynamic"
      noIcon={ false }
      noLink
    />
  );

  return (
    <>
      <TextAd mb={ 6 }/>
      <PageTitle
        title="Cross-chain tx details"
        secondRow={ titleSecondRow }
      />
      <RoutedTabs tabs={ tabs } isLoading={ query.isPlaceholderData }/>
    </>
  );
};

export default React.memo(TxCrossChain);
