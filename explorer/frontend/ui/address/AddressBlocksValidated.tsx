import { Box } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import type { SocketMessage } from 'lib/socket/types';
import type { AddressBlocksValidatedResponse } from 'types/api/address';

import config from 'configs/app';
import { getResourceKey } from 'lib/api/useApiQuery';
import useIsMounted from 'lib/hooks/useIsMounted';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { currencyUnits } from 'lib/units';
import { BLOCK } from 'stubs/block';
import { generateListStub } from 'stubs/utils';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import * as SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import AddressBlocksValidatedListItem from './blocksValidated/AddressBlocksValidatedListItem';
import AddressBlocksValidatedTableItem from './blocksValidated/AddressBlocksValidatedTableItem';

const OVERLOAD_COUNT = 75;

interface Props {
  shouldRender?: boolean;
  isQueryEnabled?: boolean;
}

const AddressBlocksValidated = ({ shouldRender = true, isQueryEnabled = true }: Props) => {
  const [ showSocketAlert, setShowSocketAlert ] = React.useState(false);
  const [ newItemsCount, setNewItemsCount ] = React.useState(0);

  const queryClient = useQueryClient();
  const router = useRouter();
  const isMounted = useIsMounted();

  const addressHash = String(router.query.hash);
  const query = useQueryWithPages({
    resourceName: 'general:address_blocks_validated',
    pathParams: { hash: addressHash },
    options: {
      enabled: isQueryEnabled,
      placeholderData: generateListStub<'general:address_blocks_validated'>(
        BLOCK,
        50,
        {
          next_page_params: {
            block_number: 9060562,
            items_count: 50,
          },
        },
      ),
    },
  });

  const handleSocketError = React.useCallback(() => {
    setShowSocketAlert(true);
  }, []);

  const handleNewSocketMessage: SocketMessage.NewBlock['handler'] = React.useCallback((payload) => {
    setShowSocketAlert(false);

    queryClient.setQueryData(
      getResourceKey('general:address_blocks_validated', { pathParams: { hash: addressHash } }),
      (prevData: AddressBlocksValidatedResponse | undefined) => {
        if (!prevData) {
          return;
        }

        if (prevData.items.length >= OVERLOAD_COUNT) {
          setNewItemsCount(prev => prev + 1);
          return prevData;
        }

        return {
          ...prevData,
          items: [ payload.block, ...prevData.items ],
        };
      });
  }, [ addressHash, queryClient ]);

  const channel = useSocketChannel({
    topic: `blocks:${ addressHash.toLowerCase() }`,
    onSocketClose: handleSocketError,
    onSocketError: handleSocketError,
    isDisabled: !addressHash || query.isPlaceholderData || query.pagination.page !== 1,
  });
  useSocketMessage({
    channel,
    event: 'new_block',
    handler: handleNewSocketMessage,
  });

  if (!isMounted || !shouldRender) {
    return null;
  }

  const content = query.data?.items ? (
    <>
      <Box hideBelow="lg">
        <TableRoot tableLayout="auto">
          <TableHeaderSticky top={ query.pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }>
            <TableRow>
              <TableColumnHeader>Block</TableColumnHeader>
              <TableColumnHeader>
                Timestamp
                <TimeFormatToggle/>
              </TableColumnHeader>
              <TableColumnHeader>Txn</TableColumnHeader>
              <TableColumnHeader>Gas used</TableColumnHeader>
              { !config.UI.views.block.hiddenFields?.total_reward && !config.features.rollup.isEnabled &&
                <TableColumnHeader isNumeric>Reward { currencyUnits.ether }</TableColumnHeader> }
            </TableRow>
          </TableHeaderSticky>
          <TableBody>
            <SocketNewItemsNotice.Desktop
              num={ newItemsCount }
              showErrorAlert={ showSocketAlert }
              type="block"
              isLoading={ query.isPlaceholderData }
            />
            { query.data.items.map((item, index) => (
              <AddressBlocksValidatedTableItem
                key={ item.height + (query.isPlaceholderData ? String(index) : '') }
                { ...item }
                page={ query.pagination.page }
                isLoading={ query.isPlaceholderData }
              />
            )) }
          </TableBody>
        </TableRoot>
      </Box>
      <Box hideFrom="lg">
        { query.pagination.page === 1 && (
          <SocketNewItemsNotice.Mobile
            num={ newItemsCount }
            showErrorAlert={ showSocketAlert }
            type="block"
            isLoading={ query.isPlaceholderData }
          />
        ) }
        { query.data.items.map((item, index) => (
          <AddressBlocksValidatedListItem
            key={ item.height + (query.isPlaceholderData ? String(index) : '') }
            { ...item }
            page={ query.pagination.page }
            isLoading={ query.isPlaceholderData }
          />
        )) }
      </Box>
    </>
  ) : null;

  const actionBar = query.pagination.isVisible ? (
    <ActionBar mt={ -6 }>
      <Pagination ml="auto" { ...query.pagination }/>
    </ActionBar>
  ) : null;

  return (
    <DataListDisplay
      isError={ query.isError }
      itemsNum={ query.data?.items.length }
      emptyText="There are no validated blocks for this address."
      actionBar={ actionBar }
    >
      { content }
    </DataListDisplay>
  );
};

export default React.memo(AddressBlocksValidated);
