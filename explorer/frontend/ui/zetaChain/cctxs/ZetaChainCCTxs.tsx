import { Box } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import React from 'react';

import { CctxStatusReduced, type CctxListItem, type ListCctxsResponse } from '@blockscout/zetachain-cctx-types';
import type { SocketMessage } from 'lib/socket/types';
import type { ZetaChainCCTXFilterParams } from 'types/client/zetaChain';
import type { PaginationParams } from 'ui/shared/pagination/types';

import { getResourceKey } from 'lib/api/useApiQuery';
import useInitialList from 'lib/hooks/useInitialList';
import useIsMobile from 'lib/hooks/useIsMobile';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import Pagination from 'ui/shared/pagination/Pagination';
import * as SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';

import ZetaChainCCTxsListItem from './ZetaChainCCTXListItem';
import ZetaChainCCTxsTable from './ZetaChainCCTxsTable';

const OVERLOAD_COUNT = 75;

type Props = {
  pagination: PaginationParams;
  top?: number;
  items?: Array<CctxListItem>;
  isPlaceholderData: boolean;
  isError: boolean;
  filters?: ZetaChainCCTXFilterParams;
  onFilterChange: <T extends keyof ZetaChainCCTXFilterParams>(field: T, val: ZetaChainCCTXFilterParams[T]) => void;
  showStatusFilter?: boolean;
  type: 'pending' | 'mined';
};

const ZetaChainCCTxs = ({
  pagination,
  top,
  items,
  isPlaceholderData,
  isError,
  filters = {},
  onFilterChange,
  showStatusFilter = true,
  type,
}: Props) => {
  const isMobile = useIsMobile();
  const queryClient = useQueryClient();
  const [ showSocketErrorAlert, setShowSocketErrorAlert ] = React.useState(false);
  const [ showOverloadNotice, setShowOverloadNotice ] = React.useState(false);

  const initialList = useInitialList({
    data: items ?? [],
    idFn: (item) => item.index,
    enabled: !isPlaceholderData,
  });

  // Socket handling for new CCTX messages
  const handleNewCCTXMessage: SocketMessage.NewZetaChainCCTXs['handler'] = React.useCallback((payload) => {
    const currentQueryKey = getResourceKey('zetachain:transactions', {
      queryParams: {
        limit: 50,
        offset: 0,
        status_reduced: type === 'pending' ? [ 'Pending' ] : [ 'Success', 'Failed' ],
        direction: 'DESC',
      },
    });

    const filteredPayload = type === 'pending' ?
      payload.filter(tx => tx.status_reduced === CctxStatusReduced.PENDING) :
      payload.filter(tx => tx.status_reduced === CctxStatusReduced.SUCCESS || tx.status_reduced === CctxStatusReduced.FAILED);

    queryClient.setQueryData(currentQueryKey, (prevData: ListCctxsResponse | undefined) => {
      if (!prevData) {
        return {
          items: filteredPayload,
          next_page_params: null,
        };
      }

      if (filteredPayload.length === 0) {
        return prevData; // No relevant transactions to add
      }

      // Create a map of existing items by index for quick lookup
      const existingItemsMap = new Map(
        prevData.items.map((item) => [ item.index, item ]),
      );

      // Update or add new items from filtered payload
      filteredPayload.forEach((newItem) => {
        existingItemsMap.set(newItem.index, newItem);
      });

      // Convert back to array, sort by last_update_timestamp (newest first)
      const mergedItems = Array.from(existingItemsMap.values())
        .sort((a, b) => Number(b.last_update_timestamp) - Number(a.last_update_timestamp));

      // Check if we've reached overload count
      if (mergedItems.length >= OVERLOAD_COUNT) {
        setShowOverloadNotice(true);
        return prevData; // Don't update the list when overloaded
      }

      return {
        ...prevData,
        items: mergedItems,
      };
    });
  }, [ queryClient, type ]);

  const handleSocketClose = React.useCallback(() => {
    setShowSocketErrorAlert(true);
  }, []);

  const handleSocketError = React.useCallback(() => {
    setShowSocketErrorAlert(true);
  }, []);

  // Socket channel for CCTX updates
  const hasFilters = Object.values(filters).some(value => value !== undefined && value !== '');

  const channel = useSocketChannel({
    topic: 'cctxs:new_cctxs',
    isDisabled: hasFilters, // Disable when filters are applied
    onSocketClose: handleSocketClose,
    onSocketError: handleSocketError,
    socketName: 'zetachain',
  });

  useSocketMessage({
    channel,
    event: 'new_cctxs',
    handler: handleNewCCTXMessage,
  });

  const content = (
    <>
      <Box hideFrom="lg">
        { pagination.page === 1 && !hasFilters && (
          <SocketNewItemsNotice.Mobile
            showErrorAlert={ showSocketErrorAlert }
            type="cross_chain_transaction"
            isLoading={ isPlaceholderData }
            num={ showOverloadNotice ? 1 : 0 }
          />
        ) }
        { (items || []).map((item, index) => (
          <ZetaChainCCTxsListItem
            key={ item.index + (isPlaceholderData ? index : '') }
            tx={ item }
            isLoading={ isPlaceholderData }
            animation={ initialList.getAnimationProp(item) }
          />
        )) }
      </Box>
      <Box hideBelow="lg">
        <ZetaChainCCTxsTable
          txs={ items ?? [] }
          top={ top || 0 }
          isLoading={ isPlaceholderData }
          filters={ filters }
          onFilterChange={ onFilterChange }
          isPlaceholderData={ isPlaceholderData }
          showStatusFilter={ showStatusFilter }
          showSocketInfo={ pagination.page === 1 && !hasFilters }
          showSocketErrorAlert={ showSocketErrorAlert }
          socketInfoNum={ showOverloadNotice ? 1 : 0 }
        />
      </Box>
    </>
  );

  const actionBar = (isMobile && pagination.isVisible) ? (
    <ActionBar mt={ -6 }>
      <Pagination ml="auto" { ...pagination }/>
    </ActionBar>
  ) : null;

  return (
    <DataListDisplay
      isError={ isError }
      itemsNum={ items?.length }
      emptyText="There are no cross chain transactions."
      hasActiveFilters={ hasFilters }
      emptyStateProps={{
        term: 'cross chain transaction',
      }}
      actionBar={ actionBar }
    >
      { content }
    </DataListDisplay>
  );
};

export default ZetaChainCCTxs;
