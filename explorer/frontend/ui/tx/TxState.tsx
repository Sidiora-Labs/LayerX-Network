import { Box, Text } from '@chakra-ui/react';
import React from 'react';

import { TX_STATE_CHANGES } from 'stubs/txStateChanges';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import TxStateList from 'ui/tx/state/TxStateList';
import TxStateTable from 'ui/tx/state/TxStateTable';

import TxPendingAlert from './TxPendingAlert';
import TxSocketAlert from './TxSocketAlert';
import type { TxQuery } from './useTxQuery';

interface Props {
  txQuery: TxQuery;
}

const TxState = ({ txQuery }: Props) => {
  const { data, isPlaceholderData, isError, pagination } = useQueryWithPages({
    resourceName: 'general:tx_state_changes',
    pathParams: { hash: txQuery.data?.hash },
    options: {
      enabled: !txQuery.isPlaceholderData && Boolean(txQuery.data?.hash) && Boolean(txQuery.data?.status),
      placeholderData: {
        items: TX_STATE_CHANGES,
        next_page_params: {
          items_count: 1,
          state_changes: null,
        },
      },
    },
  });

  if (!txQuery.isPending && !txQuery.isPlaceholderData && !txQuery.isError && !txQuery.data.status) {
    return txQuery.socketStatus ? <TxSocketAlert status={ txQuery.socketStatus }/> : <TxPendingAlert/>;
  }

  const content = data ? (
    <>
      <Box hideBelow="lg">
        <TxStateTable data={ data.items } isLoading={ isPlaceholderData } top={ pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }/>
      </Box>
      <Box hideFrom="lg">
        <TxStateList data={ data.items } isLoading={ isPlaceholderData }/>
      </Box>
    </>
  ) : null;

  const actionBar = pagination.isVisible ? (
    <ActionBar mt={ -6 } showShadow>
      <Pagination ml="auto" { ...pagination }/>
    </ActionBar>
  ) : null;

  return (
    <>
      { !isError && !txQuery.isError && (
        <Text mb={ 6 }>
          A set of information that represents the current state is updated when a transaction takes place on the network.
          The below is a summary of those changes.
        </Text>
      ) }
      <DataListDisplay
        isError={ isError || txQuery.isError }
        itemsNum={ data?.items.length }
        emptyText="There are no state changes for this transaction."
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default TxState;
