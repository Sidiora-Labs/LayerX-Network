import { Box } from '@chakra-ui/react';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import { INTERCHAIN_STATS_DAILY } from 'stubs/interchainIndexer';
import StatsWidget from 'ui/shared/stats/StatsWidget';

interface Props {}

const TransactionsCrossChainStats = (props: Props) => {
  const { data, isPlaceholderData, isError } = useApiQuery('interchainIndexer:stats_daily', {
    queryOptions: {
      placeholderData: INTERCHAIN_STATS_DAILY,
    },
  });

  if (isError || !data) {
    return null;
  }

  return (
    <Box
      display="grid"
      gridTemplateColumns="1fr"
      rowGap={ 3 }
      columnGap={ 3 }
      mb={ 6 }
      { ...props }
    >
      <StatsWidget
        label="Cross-chain txns"
        value={ Number(data.daily_messages).toLocaleString() }
        period="24h"
        isLoading={ isPlaceholderData }
      />
    </Box>
  );
};

export default React.memo(TransactionsCrossChainStats);
