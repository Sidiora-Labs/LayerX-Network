import { Box } from '@chakra-ui/react';
import React from 'react';

import { PAXEER_X_ANCHORS_ITEM } from 'stubs/paxeerXLists';
import { generateListStub } from 'stubs/utils';
import PaxeerXAnchorsListItem from 'ui/paxeerX/anchors/PaxeerXAnchorsListItem';
import PaxeerXAnchorsTable from 'ui/paxeerX/anchors/PaxeerXAnchorsTable';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import StickyPaginationWithText from 'ui/shared/StickyPaginationWithText';

const PaxeerXAnchors = () => {
  const { data, isError, isPlaceholderData, pagination } = useQueryWithPages({
    resourceName: 'general:paxeer_x_anchors',
    options: {
      placeholderData: generateListStub<'general:paxeer_x_anchors'>(
        PAXEER_X_ANCHORS_ITEM,
        50,
        {
          next_page_params: {
            items_count: 50,
            batch_number: PAXEER_X_ANCHORS_ITEM.batch_number,
          },
        },
      ),
    },
  });

  const content = data?.items ? (
    <>
      <Box hideFrom="lg">
        { data.items.map((item, index) => (
          <PaxeerXAnchorsListItem
            key={ item.checkpoint_id + (isPlaceholderData ? String(index) : '') }
            item={ item }
            isLoading={ isPlaceholderData }
          />
        )) }
      </Box>
      <Box hideBelow="lg">
        <PaxeerXAnchorsTable items={ data.items } top={ pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 } isLoading={ isPlaceholderData }/>
      </Box>
    </>
  ) : null;

  const actionBar = <StickyPaginationWithText text={ null } pagination={ pagination }/>;

  return (
    <>
      <PageTitle title="Anchor checkpoints" withTextAd/>
      <DataListDisplay
        isError={ isError }
        itemsNum={ data?.items.length }
        emptyText="There are no anchor checkpoints."
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default PaxeerXAnchors;
