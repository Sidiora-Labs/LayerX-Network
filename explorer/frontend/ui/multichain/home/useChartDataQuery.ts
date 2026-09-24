import { useQuery } from '@tanstack/react-query';

import type { ChartMarketResponse } from 'types/api/charts';

import useApiQuery from 'lib/api/useApiQuery';
import { HOMEPAGE_STATS } from 'stubs/multichain';
import { getChartData } from 'ui/home/indicators/utils/chart';

import useFetchParentChainApi from './useFetchParentChainApi';

interface Props {
  indicatorId: string;
}

export default function useChartDataQuery({ indicatorId }: Props) {

  const multichainStatsQuery = useApiQuery('multichainStats:pages_main', {
    queryOptions: {
      refetchOnMount: false,
      placeholderData: HOMEPAGE_STATS,
      enabled: indicatorId === 'daily_txs',
      select: (data) => {
        return data.new_txns_multichain_window?.chart.map((item) => ({ date: new Date(item.date), value: Number(item.value) })) || [];
      },
    },
  });

  const parentChainApiFetch = useFetchParentChainApi();

  const marketCapQuery = useQuery({
    queryKey: [ 'parent_chain', 'stats', 'charts', 'market' ],
    queryFn: () => parentChainApiFetch({ path: '/stats/charts/market' }) as Promise<ChartMarketResponse>,
    refetchOnMount: false,
    enabled: indicatorId === 'market_cap' || indicatorId === 'coin_price',
  });

  switch (indicatorId) {
    case 'daily_txs': {
      return {
        data: getChartData(indicatorId, multichainStatsQuery.data || []),
        isError: multichainStatsQuery.isError,
        isPending: multichainStatsQuery.isPending,
      };
    }
    case 'coin_price': {
      return {
        data: getChartData(indicatorId, marketCapQuery.data?.chart_data.map((item) => ({ date: new Date(item.date), value: item.closing_price })) || []),
        isError: marketCapQuery.isError,
        isPending: marketCapQuery.isPending,
      };
    }
    case 'market_cap': {
      return {
        data: getChartData(indicatorId, marketCapQuery.data?.chart_data.map((item) => ({
          date: new Date(item.date),
          value: (() => {
            if (item.market_cap !== undefined) {
              return item.market_cap;
            }

            if (item.closing_price === null) {
              return null;
            }

            return Number(item.closing_price) * Number(marketCapQuery.data?.available_supply ?? 0);
          })(),
        })) || []),
        isError: marketCapQuery.isError,
        isPending: marketCapQuery.isPending,
      };
    }

    default:
      return {
        data: [ {
          id: indicatorId,
          charts: [],
          items: [],
          name: 'Daily transactions',
        } ],
        isError: false,
        isPending: false,
      };
  }

}
