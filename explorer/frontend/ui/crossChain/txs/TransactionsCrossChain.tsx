import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import { INTERCHAIN_MESSAGE, INTERCHAIN_STATS_COMMON } from 'stubs/interchainIndexer';
import { generateListStub } from 'stubs/utils';
import { Skeleton } from 'toolkit/chakra/skeleton';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import StickyPaginationWithText from 'ui/shared/StickyPaginationWithText';

import TransactionsCrossChainContent from './TransactionsCrossChainContent';
import TransactionsCrossChainStats from './TransactionsCrossChainStats';

const TransactionsCrossChain = () => {
  const { data, isPlaceholderData, isError, pagination } = useQueryWithPages({
    resourceName: 'interchainIndexer:messages',
    options: {
      placeholderData: generateListStub<'interchainIndexer:messages'>(INTERCHAIN_MESSAGE, 50, { next_page_params: { page_token: 'token' } }),
    },
  });
  const statsQuery = useApiQuery('interchainIndexer:stats_common', {
    queryOptions: {
      placeholderData: INTERCHAIN_STATS_COMMON,
    },
  });

  const actionBarText = (
    <Skeleton loading={ statsQuery.isPlaceholderData || isPlaceholderData }>
      A total of { Number(statsQuery.data?.total_messages).toLocaleString() } cross-chain transactions found
    </Skeleton>
  );

  const actionBar = <StickyPaginationWithText text={ actionBarText } pagination={ pagination }/>;

  return (
    <>
      <TransactionsCrossChainStats/>
      <TransactionsCrossChainContent
        items={ data?.items }
        isLoading={ isPlaceholderData }
        pagination={ pagination }
        isError={ isError }
        actionBar={ actionBar }
      />
    </>
  );
};

export default React.memo(TransactionsCrossChain);
