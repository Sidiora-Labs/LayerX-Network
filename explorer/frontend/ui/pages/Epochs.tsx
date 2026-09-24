import { Box } from '@chakra-ui/react';
import React from 'react';

import { CELO_EPOCH_ITEM } from 'stubs/epoch';
import { generateListStub } from 'stubs/utils';
import EpochsListItem from 'ui/epochs/EpochsListItem';
import EpochsTable from 'ui/epochs/EpochsTable';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

const EpochsPageContent = () => {
  const epochsQuery = useQueryWithPages({
    resourceName: 'general:epochs_celo',
    options: {
      placeholderData: generateListStub<'general:epochs_celo'>(CELO_EPOCH_ITEM, 50, { next_page_params: {
        number: 1739,
        items_count: 50,
      } }),
    },
  });

  const actionBar = epochsQuery.pagination.isVisible ? (
    <ActionBar mt={ -6 }>
      <Pagination ml="auto" { ...epochsQuery.pagination }/>
    </ActionBar>
  ) : null;

  const isLoading = epochsQuery.isPlaceholderData;

  const content = (() => {
    if (epochsQuery.isError) {
      return <DataFetchAlert/>;
    }

    return epochsQuery.data?.items ? (
      <>
        <Box hideBelow="lg">
          <EpochsTable
            items={ epochsQuery.data.items }
            top={ epochsQuery.pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }
            isLoading={ isLoading }
          />
        </Box>
        <Box hideFrom="lg">
          { epochsQuery.data.items.map((item, index) => (
            <EpochsListItem
              key={ item.number + (epochsQuery.isPlaceholderData ? String(index) : '') }
              item={ item }
              isLoading={ isLoading }
            />
          )) }
        </Box>
      </>
    ) : null;
  })();

  return (
    <>
      <PageTitle title="Epochs" withTextAd/>
      <DataListDisplay
        isError={ epochsQuery.isError }
        itemsNum={ epochsQuery.data?.items?.length }
        emptyText="There are no epochs."
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default EpochsPageContent;
