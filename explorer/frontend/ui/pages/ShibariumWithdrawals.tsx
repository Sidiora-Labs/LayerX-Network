import { Box } from '@chakra-ui/react';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import { layerLabels } from 'lib/rollups/utils';
import { SHIBARIUM_WITHDRAWAL_ITEM } from 'stubs/shibarium';
import { generateListStub } from 'stubs/utils';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { rightLineArrow, nbsp } from 'toolkit/utils/htmlEntities';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import StickyPaginationWithText from 'ui/shared/StickyPaginationWithText';
import WithdrawalsListItem from 'ui/withdrawals/shibarium/WithdrawalsListItem';
import WithdrawalsTable from 'ui/withdrawals/shibarium/WithdrawalsTable';

const ShibariumWithdrawals = () => {
  const { data, isError, isPlaceholderData, pagination } = useQueryWithPages({
    resourceName: 'general:shibarium_withdrawals',
    options: {
      placeholderData: generateListStub<'general:shibarium_withdrawals'>(
        SHIBARIUM_WITHDRAWAL_ITEM,
        50,
        {
          next_page_params: {
            items_count: 50,
            block_number: 123,
          },
        },
      ),
    },
  });

  const countersQuery = useApiQuery('general:shibarium_withdrawals_count', {
    queryOptions: {
      placeholderData: 23700,
    },
  });

  const content = data?.items ? (
    <>
      <Box hideFrom="lg">
        { data.items.map(((item, index) => (
          <WithdrawalsListItem
            key={ `${ item.l2_transaction_hash }-${ index }` }
            item={ item }
            isLoading={ isPlaceholderData }
          />
        ))) }
      </Box>
      <Box hideBelow="lg">
        <WithdrawalsTable items={ data.items } top={ pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 } isLoading={ isPlaceholderData }/>
      </Box>
    </>
  ) : null;

  const text = (() => {
    if (countersQuery.isError) {
      return null;
    }

    return (
      <Skeleton loading={ countersQuery.isPlaceholderData } display="inline-block">
        A total of { countersQuery.data?.toLocaleString() } withdrawals found
      </Skeleton>
    );
  })();

  const actionBar = <StickyPaginationWithText text={ text } pagination={ pagination }/>;

  return (
    <>
      <PageTitle title={ `Withdrawals (${ layerLabels.current }${ nbsp }${ rightLineArrow }${ nbsp }${ layerLabels.parent })` } withTextAd/>
      <DataListDisplay
        isError={ isError }
        itemsNum={ data?.items.length }
        emptyText="There are no withdrawals."
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default ShibariumWithdrawals;
