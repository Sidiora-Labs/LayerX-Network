import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { Transaction } from 'types/api/transaction';

import { Badge } from 'toolkit/chakra/badge';

interface Props {
  tx: Transaction;
  isLoading?: boolean;
}

const TxWatchListTags = ({ tx, isLoading }: Props) => {
  const tags = [
    ...(tx.from?.watchlist_names || []),
    ...(tx.to?.watchlist_names || []),
  ].filter(Boolean);

  if (tags.length === 0) {
    return null;
  }

  return (
    <Flex columnGap={ 2 } rowGap={ 2 } flexWrap="wrap" overflow="hidden" maxW="100%">
      { tags.map((tag) => (
        <Badge
          key={ tag.label }
          loading={ isLoading }
          truncated
          maxW={{ base: '115px', lg: '100%' }}
          colorPalette="gray"
        >
          { tag.display_name }
        </Badge>
      )) }
    </Flex>
  );
};

export default React.memo(TxWatchListTags);
