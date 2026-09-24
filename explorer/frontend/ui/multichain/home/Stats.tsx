import { Flex } from '@chakra-ui/react';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import { HOMEPAGE_STATS } from 'stubs/multichain';
import type { HomeStatsItem } from 'ui/home/utils';
import { sortHomeStatsItems, isHomeStatsItemEnabled } from 'ui/home/utils';
import StatsWidget from 'ui/shared/stats/StatsWidget';

import ChainIndicators from './ChainIndicators';

const Stats = () => {
  const statsQuery = useApiQuery('multichainStats:pages_main', {
    queryOptions: {
      refetchOnMount: false,
      placeholderData: HOMEPAGE_STATS,
    },
  });

  const items: Array<HomeStatsItem> = React.useMemo(() => {
    return [
      statsQuery.data?.total_multichain_txns && {
        id: 'total_txs' as const,
        label: statsQuery.data.total_multichain_txns.title,
        value: Number(statsQuery.data.total_multichain_txns.value).toLocaleString(),
        icon: 'transactions' as const,
        hint: statsQuery.data.total_multichain_txns.description,
      },
      statsQuery.data?.total_multichain_addresses && {
        id: 'wallet_addresses' as const,
        label: statsQuery.data.total_multichain_addresses.title,
        value: Number(statsQuery.data.total_multichain_addresses.value).toLocaleString(),
        icon: 'wallet' as const,
        hint: statsQuery.data.total_multichain_addresses.description,
      },
    ]
      .filter(Boolean)
      .filter(isHomeStatsItemEnabled)
      .sort(sortHomeStatsItems);
  }, [ statsQuery.data ]);

  return (
    <Flex mt={ 6 } gap={ 2 } flexDirection={{ base: 'column', lg: 'row' }}>
      { items.length > 0 && (
        <Flex gap={ 2 } flexDirection={{ base: 'row', lg: 'column' }} w={{ base: '100%', lg: '270px' }}>
          { items.map((item) => (
            <StatsWidget
              key={ item.id }
              label={ item.label }
              value={ item.value }
              icon={ item.icon }
              isLoading={ statsQuery.isPlaceholderData }
              hint={ item.hint }
              w={{ base: 'calc((100% - 8px) / 2)', lg: '100%' }}
            />
          )) }
        </Flex>
      ) }
      <ChainIndicators/>
    </Flex>
  );
};

export default React.memo(Stats);
