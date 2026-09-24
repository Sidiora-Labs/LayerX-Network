import { useQuery } from '@tanstack/react-query';
import React from 'react';

import type { HomeStats } from 'types/api/stats';
import type { TChainIndicator } from 'ui/home/indicators/types';

import useApiQuery from 'lib/api/useApiQuery';
import { HOMEPAGE_STATS } from 'stubs/multichain';
import ChainIndicatorsChart from 'ui/home/indicators/ChainIndicatorsChart';
import ChainIndicatorsContainer from 'ui/home/indicators/ChainIndicatorsContainer';
import ChainIndicatorsList from 'ui/home/indicators/ChainIndicatorsList';
import { isIndicatorEnabled, sortIndicators } from 'ui/home/indicators/utils/indicators';
import IconSvg from 'ui/shared/IconSvg';

import NativeTokenIcon from '../components/NativeTokenIcon';
import useChartDataQuery from './useChartDataQuery';
import useFetchParentChainApi from './useFetchParentChainApi';

const ChainIndicators = () => {
  const statsQuery = useApiQuery('multichainStats:pages_main', {
    queryOptions: {
      refetchOnMount: false,
      placeholderData: HOMEPAGE_STATS,
    },
  });

  const parentChainApiFetch = useFetchParentChainApi();

  const parentChainStatsQuery = useQuery({
    queryKey: [ 'parent_chain', 'stats' ],
    queryFn: () => parentChainApiFetch({ path: '/stats' }) as Promise<HomeStats>,
    refetchOnMount: false,
  });

  const indicators: Array<TChainIndicator> = React.useMemo(() => ([
    statsQuery.data && {
      id: 'daily_txs' as const,
      title: statsQuery.data.new_txns_multichain_window?.info?.title || 'Daily transactions',
      value: (() => {
        if (statsQuery.data.yesterday_txns_multichain?.value) {
          return Number(statsQuery.data.yesterday_txns_multichain.value).toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' });
        }
        return 'N/A';
      })(),
      valueDiff: (() => {
        if (statsQuery.data.new_txns_multichain_window) {
          const lastDay = statsQuery.data.new_txns_multichain_window.chart[statsQuery.data.new_txns_multichain_window.chart.length - 1].value;
          const secondLastDay = statsQuery.data.new_txns_multichain_window.chart[statsQuery.data.new_txns_multichain_window.chart.length - 2].value;
          if (Number(secondLastDay) !== 0) {
            return Number(((Number(lastDay) - Number(secondLastDay)) / Number(secondLastDay) * 100).toFixed(2));
          }
        }
        return undefined;
      })(),
      // FIXME use non-navigation icon
      icon: <IconSvg name="navigation/transactions" boxSize={ 6 } bgColor="#56ACD1" borderRadius="base" color="white"/>,
      hint: (() => {
        if (statsQuery.data.new_txns_multichain_window?.info) {
          return statsQuery.data.new_txns_multichain_window.info.description;
        }
        return `Number of transactions yesterday (0:00 - 23:59 UTC). The chart displays daily transactions for the past 30 days.`;
      })(),
    },
    parentChainStatsQuery.data && {
      id: 'coin_price' as const,
      title: 'ETH price',
      value: (() => {
        if (parentChainStatsQuery.data.coin_price) {
          return '$' + Number(parentChainStatsQuery.data.coin_price).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 });
        }
        return '$N/A';
      })(),
      valueDiff: parentChainStatsQuery.data.coin_price_change_percentage ?? undefined,
      icon: <NativeTokenIcon boxSize={ 6 }/>,
      hint: 'ETH token daily price in USD.',
    },
    parentChainStatsQuery.data && {
      id: 'market_cap' as const,
      title: 'Market cap',
      value: (() => {
        if (parentChainStatsQuery.data.market_cap) {
          return '$' + Number(parentChainStatsQuery.data.market_cap).toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' });
        }
        return '$N/A';
      })(),
      icon: <IconSvg name="globe" boxSize={ 6 } bgColor="#6A5DCC" borderRadius="base" color="white"/>,
      // eslint-disable-next-line max-len
      hint: 'The total market value of a cryptocurrency\'s circulating supply. It is analogous to the free-float capitalization in the stock market. Market Cap = Current Price x Circulating Supply.',
    },
  ]
    .filter(Boolean)
    .filter(isIndicatorEnabled)
    .sort(sortIndicators)
  ), [ parentChainStatsQuery.data, statsQuery.data ]);

  const [ selectedIndicatorId, selectIndicatorId ] = React.useState(indicators[0]?.id);
  const selectedIndicator = indicators.find(({ id }) => id === selectedIndicatorId);

  const chartQuery = useChartDataQuery({ indicatorId: selectedIndicatorId });

  return (
    <ChainIndicatorsContainer>
      { selectedIndicator && (
        <ChainIndicatorsChart
          isLoading={ statsQuery.isPlaceholderData || chartQuery.isPending }
          title={ selectedIndicator.title }
          hint={ selectedIndicator.hint }
          value={ selectedIndicator?.value }
          valueDiff={ selectedIndicator?.valueDiff }
          chartQuery={ chartQuery }
        />
      ) }
      <ChainIndicatorsList
        indicators={ indicators }
        isLoading={ statsQuery.isPlaceholderData || parentChainStatsQuery.isPending }
        selectedId={ selectedIndicatorId }
        onItemClick={ selectIndicatorId }
      />
    </ChainIndicatorsContainer>
  );
};

export default React.memo(ChainIndicators);
