import { Box, Flex, Text } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs/routes';

import { Link } from 'toolkit/chakra/link';

import SearchResultsList from './SearchResultsList';
import type { QueryType, SearchQueries } from './utils';
import { SEARCH_TABS_NAMES, SEARCH_TABS_IDS } from './utils';

const MAX_ITEMS_IN_GROUP = 5;

interface Props {
  queries: SearchQueries;
}

const SearchResultsTabAll = ({ queries }: Props) => {
  return (
    <Flex flexDir="column" rowGap={ 8 }>
      { Object.entries(queries)
        .filter(([ , query ]) => query.data?.pages?.[0]?.items?.length > 0)
        .filter(([ queryType ]) => SEARCH_TABS_NAMES[queryType as QueryType])
        .map((items) => {
          const queryType = items[0] as QueryType;
          const query = items[1] as SearchQueries[QueryType];

          const hasMore = Number(query.data?.pages[0].items.length) > MAX_ITEMS_IN_GROUP;

          return (
            <Box key={ queryType }>
              <Text color="text.secondary" fontWeight={ 600 } textStyle="sm" mb={ 2 }>{ SEARCH_TABS_NAMES[queryType] }</Text>
              <SearchResultsList queryType={ queryType } query={ query } maxItems={ MAX_ITEMS_IN_GROUP }/>
              { hasMore && (
                <Link
                  pt={ 2 }
                  borderTopWidth="1px"
                  borderTopStyle="solid"
                  borderTopColor="border.divider"
                  textStyle="sm"
                  w="full"
                  href={ route({ pathname: '/search-results', query: { tab: SEARCH_TABS_IDS[queryType] } }) }
                >
                  View all
                </Link>
              ) }
            </Box>
          );
        }) }
    </Flex>
  );
};

export default React.memo(SearchResultsTabAll);
