import { Box } from '@chakra-ui/react';
import React from 'react';

import type { InterchainMessage } from '@blockscout/interchain-indexer-types';
import type { PaginationParams } from 'ui/shared/pagination/types';

import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import type { Props as DataListDisplayProps } from 'ui/shared/DataListDisplay';
import DataListDisplay from 'ui/shared/DataListDisplay';

import TransactionsCrossChainListItem from './TransactionsCrossChainListItem';
import TransactionsCrossChainTable from './TransactionsCrossChainTable';

export interface Props extends Omit<DataListDisplayProps, 'children'> {
  items?: Array<InterchainMessage>;
  isLoading?: boolean;
  pagination?: PaginationParams;
  isTableView?: boolean;
  stickyHeader?: boolean;
  currentAddress?: string;
}

const TransactionsCrossChainContent = ({ items, isLoading, pagination, isTableView, stickyHeader = true, currentAddress, ...rest }: Props) => {
  const content = items ? (
    <>
      <Box display={{ base: isTableView ? 'none' : 'block', lg: 'none' }}>
        { items.map((item, index) => (
          <TransactionsCrossChainListItem
            key={ item.message_id + (isLoading ? index : '') }
            data={ item }
            isLoading={ isLoading }
            currentAddress={ currentAddress }
          />
        )) }
      </Box>
      <Box
        display={{ base: isTableView ? 'block' : 'none', lg: 'block' }}
        overflowX={{ base: 'scroll', lg: 'initial' }}
        mx={{ base: -3, lg: 0 }}
        px={{ base: 3, lg: 0 }}
      >
        <TransactionsCrossChainTable
          data={ items }
          isLoading={ isLoading }
          top={ ACTION_BAR_HEIGHT_DESKTOP }
          stickyHeader={ stickyHeader }
          currentAddress={ currentAddress }
        />
      </Box>
    </>
  ) : null;

  return (
    <DataListDisplay
      itemsNum={ items?.length }
      emptyText="There are no cross-chain transactions."
      emptyStateProps={{
        term: 'transaction',
      }}
      { ...rest }
    >
      { content }
    </DataListDisplay>
  );
};

export default React.memo(TransactionsCrossChainContent);
