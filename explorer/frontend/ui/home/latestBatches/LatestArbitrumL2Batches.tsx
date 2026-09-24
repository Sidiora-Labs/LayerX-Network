import { Box, Flex, VStack } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
// import { AnimatePresence } from 'framer-motion';
import React from 'react';

import type { SocketMessage } from 'lib/socket/types';
import type { ArbitrumL2TxnBatchesItem } from 'types/api/arbitrumL2';

import { route } from 'nextjs-routes';

import useApiQuery, { getResourceKey } from 'lib/api/useApiQuery';
import useInitialList from 'lib/hooks/useInitialList';
import useIsMobile from 'lib/hooks/useIsMobile';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { ARBITRUM_L2_TXN_BATCHES_ITEM } from 'stubs/arbitrumL2';
import { Heading } from 'toolkit/chakra/heading';
import { Link } from 'toolkit/chakra/link';

import LatestBlocksFallback from '../fallbacks/LatestBlocksFallback';
import LatestBatchItem from './LatestBatchItem';

const LatestArbitrumL2Batches = () => {
  const isMobile = useIsMobile();
  const batchesMaxCount = isMobile ? 2 : 6;
  const queryClient = useQueryClient();

  const { data, isPlaceholderData, isError } = useApiQuery('general:homepage_arbitrum_l2_batches', {
    queryOptions: {
      placeholderData: { items: Array(batchesMaxCount).fill(ARBITRUM_L2_TXN_BATCHES_ITEM) },
    },
  });

  const initialList = useInitialList({
    data: data?.items ?? [],
    idFn: (batch) => batch.number,
    enabled: !isPlaceholderData,
  });

  const handleNewBatchMessage: SocketMessage.NewArbitrumL2Batch['handler'] = React.useCallback((payload) => {
    queryClient.setQueryData(getResourceKey('general:homepage_arbitrum_l2_batches'), (prevData: { items: Array<ArbitrumL2TxnBatchesItem> } | undefined) => {
      const newItems = prevData?.items ? [ ...prevData.items ] : [];

      if (newItems.some((batch => batch.number === payload.batch.number))) {
        return { items: newItems };
      }

      return { items: [ payload.batch, ...newItems ].sort((b1, b2) => b2.number - b1.number).slice(0, batchesMaxCount) };
    });
  }, [ queryClient, batchesMaxCount ]);

  const channel = useSocketChannel({
    topic: 'arbitrum:new_batch',
    isDisabled: isPlaceholderData || isError,
  });
  useSocketMessage({
    channel,
    event: 'new_arbitrum_batch',
    handler: handleNewBatchMessage,
  });

  const content = (() => {
    if (isError) {
      return <LatestBlocksFallback/>;
    }
    if (data && data.items.length > 0) {
      const dataToShow = data.items.slice(0, batchesMaxCount);

      return (
        <>
          <VStack gap={ 2 } mb={ 3 } overflow="hidden" alignItems="stretch">
            { dataToShow.map(((batch, index) => (
              <LatestBatchItem
                key={ batch.number + (isPlaceholderData ? String(index) : '') }
                number={ batch.number }
                timestamp={ batch.commitment_transaction.timestamp }
                txCount={ batch.transactions_count }
                isLoading={ isPlaceholderData }
                animation={ initialList.getAnimationProp(batch) }
              />
            ))) }
          </VStack>
          <Flex justifyContent="center">
            <Link textStyle="sm" href={ route({ pathname: '/batches' }) }>View all batches</Link>
          </Flex>
        </>
      );
    }
    return <Box textStyle="sm">No latest batches found.</Box>;
  })();

  return (
    <Box width={{ base: '100%', lg: '280px' }} flexShrink={ 0 }>
      <Heading level="3" mb={ 3 }>Latest batches</Heading>
      { content }
    </Box>
  );
};

export default LatestArbitrumL2Batches;
