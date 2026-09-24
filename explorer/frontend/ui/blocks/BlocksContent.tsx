import { Box } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import React from 'react';

import type { SocketMessage } from 'lib/socket/types';
import type { BlockType, BlocksResponse } from 'types/api/block';

import { route } from 'nextjs/routes';

import { getResourceKey } from 'lib/api/useApiQuery';
import { useMultichainContext } from 'lib/contexts/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { Link } from 'toolkit/chakra/link';
import BlocksList from 'ui/blocks/BlocksList';
import BlocksTable from 'ui/blocks/BlocksTable';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import IconSvg from 'ui/shared/IconSvg';
import Pagination from 'ui/shared/pagination/Pagination';
import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';
import * as SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';

const OVERLOAD_COUNT = 75;
const TABS_HEIGHT = 88;

export interface Props {
  type?: BlockType;
  query: QueryWithPagesResult<'general:blocks'> | QueryWithPagesResult<'general:optimistic_l2_txn_batch_blocks'>;
  enableSocket?: boolean;
  top?: number;
}

const BlocksContent = ({ type, query, enableSocket = true, top }: Props) => {
  const queryClient = useQueryClient();
  const isMobile = useIsMobile();
  const multichainContext = useMultichainContext();

  const [ showSocketAlert, setShowSocketAlert ] = React.useState(false);

  const [ newItemsCount, setNewItemsCount ] = React.useState(0);

  const handleNewBlockMessage: SocketMessage.NewBlock['handler'] = React.useCallback((payload) => {
    const queryKey = getResourceKey('general:blocks', {
      queryParams: { type },
      chainId: multichainContext?.chain?.id,
    });

    queryClient.setQueryData(queryKey, (prevData: BlocksResponse | undefined) => {
      const shouldAddToList = !type || type === payload.block.type;

      if (!prevData) {
        return {
          items: shouldAddToList ? [ payload.block ] : [],
          next_page_params: null,
        };
      }

      if (!shouldAddToList || prevData.items.some((block => block.height === payload.block.height))) {
        return prevData;
      }

      if (prevData.items.length >= OVERLOAD_COUNT) {
        setNewItemsCount(prev => prev + 1);
        return prevData;
      }

      const newItems = [ payload.block, ...prevData.items ].sort((b1, b2) => b2.height - b1.height);
      return { ...prevData, items: newItems };
    });
  }, [ multichainContext?.chain?.id, queryClient, type ]);

  const handleSocketClose = React.useCallback(() => {
    setShowSocketAlert(true);
  }, []);

  const handleSocketError = React.useCallback(() => {
    setShowSocketAlert(true);
  }, []);

  const channel = useSocketChannel({
    topic: 'blocks:new_block',
    onSocketClose: handleSocketClose,
    onSocketError: handleSocketError,
    isDisabled: query.isPlaceholderData || query.isError || query.pagination.page !== 1 || !enableSocket,
  });
  useSocketMessage({
    channel,
    event: 'new_block',
    handler: handleNewBlockMessage,
  });

  const chainData = multichainContext?.chain;

  const content = query.data?.items ? (
    <>
      <Box hideFrom="lg">
        { query.pagination.page === 1 && enableSocket && (
          <SocketNewItemsNotice.Mobile
            num={ newItemsCount }
            showErrorAlert={ showSocketAlert }
            type="block"
            isLoading={ query.isPlaceholderData }
          />
        ) }
        <BlocksList data={ query.data.items } isLoading={ query.isPlaceholderData } page={ query.pagination.page } chainData={ chainData }/>
      </Box>
      <Box hideBelow="lg">
        <BlocksTable
          data={ query.data.items }
          top={ top || (query.pagination.isVisible ? TABS_HEIGHT : 0) }
          page={ query.pagination.page }
          isLoading={ query.isPlaceholderData }
          showSocketInfo={ query.pagination.page === 1 && enableSocket }
          socketInfoNum={ newItemsCount }
          showSocketErrorAlert={ showSocketAlert }
          chainData={ chainData }
        />
      </Box>
    </>
  ) : null;

  const actionBar = isMobile ? (
    <ActionBar mt={ -6 }>
      <Link href={ route({ pathname: '/block/countdown' }, multichainContext) }>
        <IconSvg name="hourglass" boxSize={ 5 } mr={ 2 }/>
        <span>Block countdown</span>
      </Link>
      <Pagination ml="auto" { ...query.pagination }/>
    </ActionBar>
  ) : null;

  return (
    <DataListDisplay
      isError={ query.isError }
      itemsNum={ query.data?.items?.length }
      emptyText="There are no blocks."
      actionBar={ actionBar }
    >
      { content }
    </DataListDisplay>
  );
};

export default React.memo(BlocksContent);
