import { Box } from '@chakra-ui/react';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import { INTEROP_MESSAGE } from 'stubs/interop';
import { generateListStub } from 'stubs/utils';
import { Skeleton } from 'toolkit/chakra/skeleton';
import InteropMessagesListItem from 'ui/interopMessages/InteropMessagesListItem';
import InteropMessagesTable from 'ui/interopMessages/InteropMessagesTable';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import StickyPaginationWithText from 'ui/shared/StickyPaginationWithText';

const InteropMessages = () => {
  const interopMessagesQuery = useQueryWithPages({
    resourceName: 'general:optimistic_l2_interop_messages',
    options: {
      placeholderData: generateListStub<'general:optimistic_l2_interop_messages'>(INTEROP_MESSAGE, 50, { next_page_params: {
        init_transaction_hash: '',
        items_count: 50,
        timestamp: 0,
      } }),
    },
  });

  const countQuery = useApiQuery('general:optimistic_l2_interop_messages_count', {
    queryOptions: {
      placeholderData: 1927029,
    },
  });

  const text = (() => {
    if (countQuery.isError) {
      return null;
    }

    return (
      <Skeleton loading={ countQuery.isPlaceholderData }>
        A total of { countQuery.data?.toLocaleString() } messages found
      </Skeleton>
    );
  })();

  const actionBar = <StickyPaginationWithText text={ text } pagination={ interopMessagesQuery.pagination }/>;

  const content = (
    <>
      <Box hideFrom="lg">
        { interopMessagesQuery.data?.items.map((item, index) => (
          <InteropMessagesListItem
            key={ item.init_transaction_hash + (interopMessagesQuery.isPlaceholderData ? index : '') }
            item={ item }
            isLoading={ interopMessagesQuery.isPlaceholderData }
          />
        )) }
      </Box>
      <Box hideBelow="lg">
        <InteropMessagesTable
          items={ interopMessagesQuery.data?.items }
          top={ interopMessagesQuery.pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }
          isLoading={ interopMessagesQuery.isPlaceholderData }
        />
      </Box>
    </>
  );

  return (
    <>
      <PageTitle
        title="Interop messages"
        withTextAd
      />
      <DataListDisplay
        isError={ interopMessagesQuery.isError }
        itemsNum={ interopMessagesQuery.data?.items.length }
        emptyText="There are no interop messages."
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default InteropMessages;
