import type * as multichain from '@blockscout/multichain-aggregator-types';
import type * as stats from '@blockscout/stats-types';

import { averageGasPrice } from 'mocks/stats/line';

import { chainA, chainB, chainC } from './chains';

export const mainPageStats: stats.MainPageMultichainStats = {
  total_multichain_txns: {
    id: 'totalMultichainTxns',
    value: '741682908',
    title: 'Total transactions',
    units: undefined,
    description: 'Number of transactions across all chains in the cluster',
  },
  total_multichain_addresses: {
    id: 'totalMultichainAddresses',
    value: '153519638',
    title: 'Total addresses',
    units: undefined,
    description: 'Number of addresses across all chains in the cluster',
  },
  new_txns_multichain_window: {
    chart: averageGasPrice.chart,
    info: {
      id: 'newTxnsMultichainWindow',
      title: 'New transactions',
      description: 'Number of new transactions across all chains in the cluster',
      resolutions: [ 'DAY' ],
    },
  },
  yesterday_txns_multichain: {
    id: 'yesterdayTxnsMultichain',
    value: '1026175',
    title: 'Yesterday txns',
    units: undefined,
    description: 'Number of transactions yesterday (0:00 - 23:59 UTC) across all chains in the cluster',
  },
};

export const chainMetrics: multichain.ListChainMetricsResponse = {
  items: [
    {
      chain_id: chainA.id,
      active_accounts: {
        current_full_week: '1000',
        previous_full_week: '900',
        wow_diff_percent: '10',
      },
    },
    {
      chain_id: chainB.id,
      active_accounts: undefined,
      tps: '42',
      new_addresses: {
        current_full_week: '200',
        previous_full_week: '210',
        wow_diff_percent: '-10',
      },
    },
    {
      chain_id: chainC.id,
      active_accounts: undefined,
      tps: '3.21',
      daily_transactions: {
        current_full_week: '3343480',
        previous_full_week: '3343480',
        wow_diff_percent: '0',
      },
    },
  ],
};
