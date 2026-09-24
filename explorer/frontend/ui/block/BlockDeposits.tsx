import { Box } from '@chakra-ui/react';
import React from 'react';

import BeaconChainDepositsList from 'ui/deposits/beaconChain/BeaconChainDepositsList';
import BeaconChainDepositsTable from 'ui/deposits/beaconChain/BeaconChainDepositsTable';
import DataListDisplay from 'ui/shared/DataListDisplay';
import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';

type Props = {
  blockDepositsQuery: QueryWithPagesResult<'general:block_deposits'>;
};
const TABS_HEIGHT = 88;

const BlockDeposits = ({ blockDepositsQuery }: Props) => {
  const content = blockDepositsQuery.data?.items ? (
    <>
      <Box hideFrom="lg">
        <BeaconChainDepositsList
          items={ blockDepositsQuery.data.items }
          isLoading={ blockDepositsQuery.isPlaceholderData }
          view="block"
        />
      </Box>
      <Box hideBelow="lg">
        <BeaconChainDepositsTable
          items={ blockDepositsQuery.data.items }
          isLoading={ blockDepositsQuery.isPlaceholderData }
          top={ blockDepositsQuery.pagination.isVisible ? TABS_HEIGHT : 0 }
          view="block"
        />
      </Box>
    </>
  ) : null ;

  return (
    <DataListDisplay
      isError={ blockDepositsQuery.isError }
      itemsNum={ blockDepositsQuery.data?.items?.length }
      emptyText="There are no deposits for this block."
    >
      { content }
    </DataListDisplay>
  );
};

export default BlockDeposits;
