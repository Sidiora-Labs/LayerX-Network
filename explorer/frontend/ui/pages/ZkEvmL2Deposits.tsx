import { Box } from '@chakra-ui/react';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import { layerLabels } from 'lib/rollups/utils';
import { generateListStub } from 'stubs/utils';
import { ZKEVM_DEPOSITS_ITEM } from 'stubs/zkEvmL2';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { rightLineArrow, nbsp } from 'toolkit/utils/htmlEntities';
import ZkEvmL2DepositsListItem from 'ui/deposits/zkEvmL2/ZkEvmL2DepositsListItem';
import ZkEvmL2DepositsTable from 'ui/deposits/zkEvmL2/ZkEvmL2DepositsTable';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import StickyPaginationWithText from 'ui/shared/StickyPaginationWithText';

const ZkEvmL2Deposits = () => {
  const { data, isError, isPlaceholderData, pagination } = useQueryWithPages({
    resourceName: 'general:zkevm_l2_deposits',
    options: {
      placeholderData: generateListStub<'general:zkevm_l2_deposits'>(
        ZKEVM_DEPOSITS_ITEM,
        50,
        { next_page_params: { items_count: 50, index: 1 } },
      ),
    },
  });

  const countersQuery = useApiQuery('general:zkevm_l2_deposits_count', {
    queryOptions: {
      placeholderData: 1927029,
    },
  });

  const content = data?.items ? (
    <>
      <Box hideFrom="lg">
        { data.items.map(((item, index) => (
          <ZkEvmL2DepositsListItem
            key={ String(item.index) + (isPlaceholderData ? index : '') }
            isLoading={ isPlaceholderData }
            item={ item }
          />
        ))) }
      </Box>
      <Box hideBelow="lg">
        <ZkEvmL2DepositsTable items={ data.items } top={ pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 } isLoading={ isPlaceholderData }/>
      </Box>
    </>
  ) : null;

  const text = (() => {
    if (countersQuery.isError) {
      return null;
    }

    return (
      <Skeleton
        loading={ countersQuery.isPlaceholderData }
        display="inline-block"
      >
        A total of { countersQuery.data?.toLocaleString() } deposits found
      </Skeleton>
    );
  })();

  const actionBar = <StickyPaginationWithText text={ text } pagination={ pagination }/>;

  return (
    <>
      <PageTitle title={ `Deposits (${ layerLabels.parent }${ nbsp }${ rightLineArrow }${ nbsp }${ layerLabels.current })` } withTextAd/>
      <DataListDisplay
        isError={ isError }
        itemsNum={ data?.items.length }
        emptyText="There are no deposits."
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default ZkEvmL2Deposits;
