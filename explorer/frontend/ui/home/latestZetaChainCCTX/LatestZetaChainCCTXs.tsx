import { Box, Flex, Text } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import React from 'react';

import { Direction } from '@blockscout/zetachain-cctx-types';
import type { ListCctxsResponse } from '@blockscout/zetachain-cctx-types';
import type { SocketMessage } from 'lib/socket/types';

import { route } from 'nextjs-routes';

import useApiQuery, { getResourceKey } from 'lib/api/useApiQuery';
import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import useInitialList from 'lib/hooks/useInitialList';
import useIsMobile from 'lib/hooks/useIsMobile';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { generateListStub } from 'stubs/utils';
import { ZETA_CHAIN_CCTX_LIST_ITEM } from 'stubs/zetaChainCCTX';
import { Link } from 'toolkit/chakra/link';
import SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';
import ZetaChainCCTXListItem from 'ui/zetaChain/cctxs/ZetaChainCCTXListItem';

import LatestTxsFallback from '../fallbacks/LatestTxsFallback';
import LatestZetaChainCCTXItem from './LatestZetaChainCCTXItem';

const LatestZetaChainCCTXs = () => {
  const isMobile = useIsMobile();
  const txsCount = isMobile ? 3 : 8;
  const { data, isPlaceholderData, isError } = useApiQuery('zetachain:transactions', {
    queryOptions: {
      placeholderData: generateListStub<'zetachain:transactions'>(
        ZETA_CHAIN_CCTX_LIST_ITEM,
        50,
        { next_page_params: { page_key: 0, limit: 0, direction: Direction.DESC } },
      ),
    },
    queryParams: {
      limit: txsCount,
      offset: 0,
      direction: 'DESC',
    },
  });

  const initialList = useInitialList({
    data: data?.items ?? [],
    idFn: (tx) => tx.index,
    enabled: !isPlaceholderData,
  });

  const queryClient = useQueryClient();

  const [ showSocketErrorAlert, setShowSocketErrorAlert ] = React.useState(false);

  const handleSocketClose = React.useCallback(() => {
    setShowSocketErrorAlert(true);
  }, []);

  const handleSocketError = React.useCallback(() => {
    setShowSocketErrorAlert(true);
  }, []);

  const handleNewCCTXMessage: SocketMessage.NewZetaChainCCTXs['handler'] = React.useCallback((payload) => {
    queryClient.setQueryData(
      getResourceKey('zetachain:transactions', {
        queryParams: {
          limit: txsCount,
          offset: 0,
          direction: 'DESC',
        },
      }),
      (prevData: ListCctxsResponse | undefined) => {
        if (!prevData) {
          return {
            items: payload,
            next_page_params: null,
          };
        }

        const existingItemsMap = new Map(
          prevData.items.map(item => [ item.index, item ]),
        );

        payload.forEach(newItem => {
          existingItemsMap.set(newItem.index, newItem);
        });

        const mergedItems = Array.from(existingItemsMap.values())
          .sort((a, b) => Number(b.last_update_timestamp) - Number(a.last_update_timestamp))
          .slice(0, txsCount);

        return {
          ...prevData,
          items: mergedItems,
        };
      },
    );
  }, [ queryClient, txsCount ]);

  const channel = useSocketChannel({
    topic: 'cctxs:new_cctxs',
    isDisabled: Boolean(isPlaceholderData),
    socketName: 'zetachain',
    onSocketClose: handleSocketClose,
    onSocketError: handleSocketError,
  });

  useSocketMessage({
    channel,
    event: 'new_cctxs',
    handler: handleNewCCTXMessage,
  });

  if (isError) {
    return <LatestTxsFallback/>;
  }

  if (data) {
    const cctxsUrl = route({ pathname: '/txs', query: { tab: 'cctx' } });
    return (
      <>
        <SocketNewItemsNotice
          type="cross_chain_transaction"
          isLoading={ isPlaceholderData }
          showErrorAlert={ showSocketErrorAlert }
          borderBottomRadius={ 0 }
        />
        <Box mb={ 3 } display={{ base: 'block', lg: 'none' }}>
          { data.items.slice(0, txsCount).map(((tx, index) => (
            <ZetaChainCCTXListItem
              key={ tx.index + (isPlaceholderData ? index : '') }
              tx={ tx }
              isLoading={ isPlaceholderData }
              animation={ initialList.getAnimationProp(tx) }
            />
          ))) }
        </Box>
        <AddressHighlightProvider>
          <Box mb={ 3 } display={{ base: 'none', lg: 'block' }}>
            { data.items.slice(0, txsCount).map(((tx, index) => (
              <LatestZetaChainCCTXItem
                key={ tx.index + (isPlaceholderData ? index : '') }
                tx={ tx }
                isLoading={ isPlaceholderData }
                animation={ initialList.getAnimationProp(tx) }
              />
            ))) }
          </Box>
        </AddressHighlightProvider>
        <Flex justifyContent="center">
          <Link textStyle="sm" href={ cctxsUrl }>View all cross chain transactions</Link>
        </Flex>
      </>
    );
  }

  return <Text>No latest cross chain transactions found.</Text>;
};

export default LatestZetaChainCCTXs;
