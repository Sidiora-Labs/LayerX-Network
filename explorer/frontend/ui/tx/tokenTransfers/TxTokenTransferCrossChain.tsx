import { Box } from '@chakra-ui/react';
import React from 'react';

import TokenTransfersCrossChainListItem from 'ui/crossChain/transfers/TokenTransfersCrossChainListItem';
import TokenTransfersCrossChainTable from 'ui/crossChain/transfers/TokenTransfersCrossChainTable';
import { getItemKey } from 'ui/crossChain/transfers/utils';
import DataListDisplay from 'ui/shared/DataListDisplay';
import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';

import type { TxQuery } from '../useTxQuery';

interface Props {
  txQuery: TxQuery;
  crossChainQuery: QueryWithPagesResult<'interchainIndexer:tx_transfers'>;
  isLoading?: boolean;
  tableTop?: number;
}

const TxTokenTransferCrossChain = ({ txQuery, crossChainQuery, isLoading, tableTop }: Props) => {
  const content = crossChainQuery.data?.items ? (
    <>
      <Box hideFrom="lg">
        { crossChainQuery.data.items.map((item, index) => (
          <TokenTransfersCrossChainListItem
            key={ getItemKey(item, crossChainQuery.isPlaceholderData ? index : undefined) }
            data={ item }
            isLoading={ isLoading || crossChainQuery.isPlaceholderData }
          />
        )) }
      </Box>
      <Box hideBelow="lg">
        <TokenTransfersCrossChainTable
          data={ crossChainQuery.data.items }
          isLoading={ isLoading || crossChainQuery.isPlaceholderData }
          top={ tableTop }
        />
      </Box>
    </>
  ) : null;

  return (
    <DataListDisplay
      isError={ txQuery.isError || crossChainQuery.isError }
      itemsNum={ crossChainQuery.data?.items.length }
      emptyText="There are no cross-chain token transfers."
      emptyStateProps={{
        term: 'token transfer',
      }}
    >
      { content }
    </DataListDisplay>
  );
};

export default React.memo(TxTokenTransferCrossChain);
