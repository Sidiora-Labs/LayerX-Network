import { Box } from '@chakra-ui/react';
import React from 'react';

import type { TokenTransfer } from 'types/api/tokenTransfer';

import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';
import TokenTransferList from 'ui/shared/TokenTransfer/TokenTransferList';
import TokenTransferTable from 'ui/shared/TokenTransfer/TokenTransferTable';

import type { TxQuery } from '../useTxQuery';

interface Props {
  txQuery: TxQuery;
  tokenTransferQuery: QueryWithPagesResult<'general:tx_token_transfers'>;
  tokenTransferFilter?: (data: TokenTransfer) => boolean;
  numActiveFilters: number;
}

const TxTokenTransferLocal = ({ txQuery, tokenTransferQuery, tokenTransferFilter, numActiveFilters }: Props) => {

  let items: Array<TokenTransfer> = [];

  if (tokenTransferQuery.data?.items) {
    if (tokenTransferQuery.isPlaceholderData) {
      items = tokenTransferQuery.data?.items;
    } else {
      items = tokenTransferFilter ? tokenTransferQuery.data.items.filter(tokenTransferFilter) : tokenTransferQuery.data.items;
    }
  }

  const content = tokenTransferQuery.data?.items ? (
    <>
      <Box hideBelow="lg">
        <TokenTransferTable data={ items } top={ ACTION_BAR_HEIGHT_DESKTOP } isLoading={ tokenTransferQuery.isPlaceholderData }/>
      </Box>
      <Box hideFrom="lg">
        <TokenTransferList data={ items } isLoading={ tokenTransferQuery.isPlaceholderData }/>
      </Box>
    </>
  ) : null;

  return (
    <DataListDisplay
      isError={ txQuery.isError || tokenTransferQuery.isError }
      itemsNum={ items.length }
      emptyText="There are no token transfers."
      hasActiveFilters={ Boolean(numActiveFilters) }
      emptyStateProps={{
        term: 'token transfer',
      }}
    >
      { content }
    </DataListDisplay>
  );
};

export default React.memo(TxTokenTransferLocal);
