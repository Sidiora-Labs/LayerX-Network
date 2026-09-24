import { Box } from '@chakra-ui/react';
import React from 'react';

import { PAXEER_X_RECEIPTS_ITEM } from 'stubs/paxeerXLists';
import { generateListStub } from 'stubs/utils';
import PaxeerXReceiptsListItem from 'ui/paxeerX/receipts/PaxeerXReceiptsListItem';
import PaxeerXReceiptsTable from 'ui/paxeerX/receipts/PaxeerXReceiptsTable';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import StickyPaginationWithText from 'ui/shared/StickyPaginationWithText';

const PaxeerXReceipts = () => {
  const { data, isError, isPlaceholderData, pagination } = useQueryWithPages({
    resourceName: 'general:paxeer_x_receipts',
    options: {
      placeholderData: generateListStub<'general:paxeer_x_receipts'>(
        PAXEER_X_RECEIPTS_ITEM,
        50,
        {
          next_page_params: {
            items_count: 50,
            id: PAXEER_X_RECEIPTS_ITEM.id,
          },
        },
      ),
    },
  });

  const content = data?.items ? (
    <>
      <Box hideFrom="lg">
        { data.items.map((item, index) => (
          <PaxeerXReceiptsListItem
            key={ item.id + (isPlaceholderData ? String(index) : '') }
            item={ item }
            isLoading={ isPlaceholderData }
          />
        )) }
      </Box>
      <Box hideBelow="lg">
        <PaxeerXReceiptsTable items={ data.items } top={ pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 } isLoading={ isPlaceholderData }/>
      </Box>
    </>
  ) : null;

  const actionBar = <StickyPaginationWithText text={ null } pagination={ pagination }/>;

  return (
    <>
      <PageTitle title="Kernel receipts" withTextAd/>
      <DataListDisplay
        isError={ isError }
        itemsNum={ data?.items.length }
        emptyText="There are no kernel receipts."
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default PaxeerXReceipts;
