import type { TimeChartData } from 'toolkit/components/charts/types';
import type { ChainIndicatorId } from 'types/homepage';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';

import { getChartData } from './utils/chart';

const rollupFeature = config.features.rollup;
const isOptimisticRollup = rollupFeature.isEnabled && rollupFeature.type === 'optimistic';
const isArbitrumRollup = rollupFeature.isEnabled && rollupFeature.type === 'arbitrum';

const isStatsFeatureEnabled = config.features.stats.isEnabled;

export type UseFetchChartDataResult = {
  isError: boolean;
  isPending: boolean;
  data: TimeChartData;
};

export default function useChartDataQuery(indicatorId: ChainIndicatorId): UseFetchChartDataResult {
  const statsDailyTxsQuery = useApiQuery('stats:pages_main', {
    queryOptions: {
      refetchOnMount: false,
      enabled: isStatsFeatureEnabled && indicatorId === 'daily_txs',
      select: (data) => data.daily_new_transactions?.chart.map((item) => ({ date: new Date(item.date), value: Number(item.value) })) || [],
    },
  });

  const statsDailyOperationalTxsQuery = useApiQuery('stats:pages_main', {
    queryOptions: {
      refetchOnMount: false,
      enabled: isStatsFeatureEnabled && indicatorId === 'daily_operational_txs',
      select: (data) => {
        if (isArbitrumRollup) {
          return data.daily_new_operational_transactions?.chart.map((item) => ({ date: new Date(item.date), value: Number(item.value) })) || [];
        } else if (isOptimisticRollup) {
          return data.op_stack_daily_new_operational_transactions?.chart.map((item) => ({ date: new Date(item.date), value: Number(item.value) })) || [];
        }
        return [];
      },
    },
  });

  const apiDailyTxsQuery = useApiQuery('general:stats_charts_txs', {
    queryOptions: {
      refetchOnMount: false,
      enabled: !isStatsFeatureEnabled && indicatorId === 'daily_txs',
      select: (data) => data.chart_data.map((item) => ({ date: new Date(item.date), value: item.transactions_count })),
    },
  });

  const coinPriceQuery = useApiQuery('general:stats_charts_market', {
    queryOptions: {
      refetchOnMount: false,
      enabled: indicatorId === 'coin_price',
      select: (data) => data.chart_data.map((item) => ({ date: new Date(item.date), value: item.closing_price })),
    },
  });

  const secondaryCoinPriceQuery = useApiQuery('general:stats_charts_secondary_coin_price', {
    queryOptions: {
      refetchOnMount: false,
      enabled: indicatorId === 'secondary_coin_price',
      select: (data) => data.chart_data.map((item) => ({ date: new Date(item.date), value: item.closing_price })),
    },
  });

  const marketCapQuery = useApiQuery('general:stats_charts_market', {
    queryOptions: {
      refetchOnMount: false,
      enabled: indicatorId === 'market_cap',
      select: (data) => data.chart_data.map((item) => (
        {
          date: new Date(item.date),
          value: (() => {
            if (item.market_cap !== undefined) {
              return item.market_cap;
            }

            if (item.closing_price === null) {
              return null;
            }

            return Number(item.closing_price) * Number(data.available_supply);
          })(),
        })),
    },
  });

  const tvlQuery = useApiQuery('general:stats_charts_market', {
    queryOptions: {
      refetchOnMount: false,
      enabled: indicatorId === 'tvl',
      select: (data) => data.chart_data.map((item) => (
        {
          date: new Date(item.date),
          value: item.tvl !== undefined ? item.tvl : 0,
        })),
    },
  });

  switch (indicatorId) {
    case 'daily_txs': {
      const query = isStatsFeatureEnabled ? statsDailyTxsQuery : apiDailyTxsQuery;
      return {
        data: getChartData(indicatorId, query.data || []),
        isError: query.isError,
        isPending: query.isPending,
      };
    }
    case 'daily_operational_txs': {
      return {
        data: getChartData(indicatorId, statsDailyOperationalTxsQuery.data || []),
        isError: statsDailyOperationalTxsQuery.isError,
        isPending: statsDailyOperationalTxsQuery.isPending,
      };
    }
    case 'coin_price': {
      return {
        data: getChartData(indicatorId, coinPriceQuery.data || []),
        isError: coinPriceQuery.isError,
        isPending: coinPriceQuery.isPending,
      };
    }
    case 'secondary_coin_price': {
      return {
        data: getChartData(indicatorId, secondaryCoinPriceQuery.data || []),
        isError: secondaryCoinPriceQuery.isError,
        isPending: secondaryCoinPriceQuery.isPending,
      };
    }
    case 'market_cap': {
      return {
        data: getChartData(indicatorId, marketCapQuery.data || []),
        isError: marketCapQuery.isError,
        isPending: marketCapQuery.isPending,
      };
    }
    case 'tvl': {
      return {
        data: getChartData(indicatorId, tvlQuery.data || []),
        isError: tvlQuery.isError,
        isPending: tvlQuery.isPending,
      };
    }
  }
}
