import type * as stats from '@blockscout/stats-types';

import { averageGasPrice } from './line';

export const base: stats.MainPageStats = {
  average_block_time: {
    id: 'averageBlockTime',
    value: '14.909090909090908',
    title: 'Average block time',
    units: 's',
    description: 'Average time taken in seconds for a block to be included in the blockchain',
  },
  total_addresses: {
    id: 'totalAddresses',
    value: '113606435',
    title: 'Total addresses',
    description: 'Number of addresses that participated in the blockchain',
  },
  total_blocks: {
    id: 'totalBlocks',
    value: '7660515',
    title: 'Total blocks',
    description: 'Number of blocks over all time',
  },
  total_transactions: {
    id: 'totalTxns',
    value: '411264599',
    title: 'Total txns',
    description: 'All transactions including pending, dropped, replaced, failed transactions',
  },
  yesterday_transactions: {
    id: 'yesterdayTxns',
    value: '213019',
    title: 'Yesterday txns',
    description: 'Number of transactions yesterday (0:00 - 23:59 UTC)',
  },
  total_operational_transactions: {
    id: 'totalOperationalTxns',
    value: '403598877',
    title: 'Total operational txns',
    description: '\'Total txns\' without block creation transactions',
  },
  yesterday_operational_transactions: {
    id: 'yesterdayOperationalTxns',
    value: '210852',
    title: 'Yesterday operational txns',
    description: 'Number of transactions yesterday (0:00 - 23:59 UTC) without block creation transactions',
  },
  daily_new_transactions: {
    chart: averageGasPrice.chart,
    info: {
      id: 'newTxnsWindow',
      title: 'Daily transactions',
      description: 'The chart displays daily transactions for the past 30 days',
      resolutions: [
        'DAY',
      ],
    },
  },
  daily_new_operational_transactions: {
    chart: averageGasPrice.chart,
    info: {
      id: 'newOperationalTxnsWindow',
      title: 'Daily operational transactions',
      description: 'The chart displays daily transactions for the past 30 days (without block creation transactions)',
      resolutions: [
        'DAY',
      ],
    },
  },
};
