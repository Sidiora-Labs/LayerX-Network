import type * as stats from '@blockscout/stats-types';
import type { HomeStats } from 'types/api/stats';

export const HOMEPAGE_STATS: HomeStats = {
  average_block_time: 14346,
  coin_price: '1807.68',
  coin_price_change_percentage: 42,
  gas_prices: {
    average: {
      fiat_price: '1.01',
      price: 20.41,
      time: 12283,
      base_fee: 2.22222,
      priority_fee: 12.424242,
    },
    fast: {
      fiat_price: '1.26',
      price: 25.47,
      time: 9321,
      base_fee: 4.44444,
      priority_fee: 22.242424,
    },
    slow: {
      fiat_price: '0.97',
      price: 19.55,
      time: 24543,
      base_fee: 1.11111,
      priority_fee: 7.8909,
    },
  },
  gas_price_updated_at: '2022-11-11T11:09:49.051171Z',
  gas_prices_update_in: 300000,
  gas_used_today: '0',
  market_cap: '0',
  network_utilization_percentage: 22.56,
  static_gas_price: null,
  total_addresses: '28634064',
  total_blocks: '8940150',
  total_gas_used: '0',
  total_transactions: '193823272',
  transactions_today: '0',
  tvl: '1767425.102766552',
};

const STATS_CHART_INFO: stats.LineChartInfo = {
  id: 'chart_0',
  title: 'Average transaction fee',
  description: 'The average amount in ETH spent per transaction',
  units: 'ETH',
  resolutions: [ 'DAY', 'MONTH' ],
};

export const STATS_CHARTS_SECTION: stats.LineChartSection = {
  id: 'placeholder',
  title: 'Placeholder',
  charts: [
    STATS_CHART_INFO,
    {
      id: 'chart_1',
      title: 'Transactions fees',
      description: 'Amount of tokens paid as fees',
      units: 'ETH',
      resolutions: [ 'DAY', 'MONTH' ],
    },
    {
      id: 'chart_2',
      title: 'New transactions',
      description: 'New transactions number',
      units: undefined,
      resolutions: [ 'DAY', 'MONTH' ],
    },
    {
      id: 'chart_3',
      title: 'Transactions growth',
      description: 'Cumulative transactions number',
      units: undefined,
      resolutions: [ 'DAY', 'MONTH' ],
    },
  ],
};

export const STATS_CHARTS_SECTION_GAS: stats.LineChartSection = {
  id: 'gas',
  title: 'Gas',
  charts: [ {
    id: 'averageGasPrice',
    title: 'Average gas price',
    description: 'Average gas price',
    units: 'ETH',
    resolutions: [ 'DAY', 'MONTH' ],
  } ],
};

export const STATS_CHARTS = {
  sections: [ STATS_CHARTS_SECTION ],
};

export const STATS_COUNTER: stats.Counter = {
  id: 'stub',
  value: '9074405',
  title: 'Placeholder Counter',
  description: 'Placeholder description',
  units: '',
};

export const HOMEPAGE_STATS_MICROSERVICE: stats.MainPageStats = {
  average_block_time: STATS_COUNTER,
  total_addresses: STATS_COUNTER,
  total_blocks: STATS_COUNTER,
  total_transactions: STATS_COUNTER,
  yesterday_transactions: STATS_COUNTER,
  total_operational_transactions: STATS_COUNTER,
  yesterday_operational_transactions: STATS_COUNTER,
  daily_new_transactions: {
    chart: [],
    info: { ...STATS_CHART_INFO, title: 'Daily transactions' },
  },
  daily_new_operational_transactions: {
    chart: [],
    info: { ...STATS_CHART_INFO, title: 'Daily op txns' },
  },
};
