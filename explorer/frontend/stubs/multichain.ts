import type * as multichain from '@blockscout/multichain-aggregator-types';
import type * as stats from '@blockscout/stats-types';
import type { AddressTokenItem } from 'types/client/multichainAggregator';

import { ADDRESS_HASH } from './addressParams';

export const ADDRESS: multichain.GetAddressResponse = {
  hash: ADDRESS_HASH,
  chain_infos: {
    '420120000': {
      coin_balance: '1000000000000000000000000',
      is_contract: true,
      is_verified: true,
    },
  },
  has_tokens: true,
  has_interop_message_transfers: false,
  coin_balance: '1000000000000000000000000',
  exchange_rate: '123.456',
  domains: [],
};

export const ADDRESS_PORTFOLIO: multichain.GetAddressPortfolioResponse = {
  portfolio: {
    total_value: '370373.20',
    chain_values: {
      '420120000': '1000000000000000000000000',
      '100': '9.58848343977132465600',
      '130': '56.45557826318489510620',
      '137': '54.3384235217602434',
    },
  },
};

export const TOKEN: AddressTokenItem = {
  token: {
    address_hash: ADDRESS_HASH,
    circulating_market_cap: '10.01',
    decimals: '6',
    holders_count: '141268',
    icon_url: null,
    name: 'SR2USD',
    symbol: 'sR2USD',
    total_supply: '943969542711126',
    type: 'ERC-20' as const,
    exchange_rate: '123.456',
    reputation: null,
    chain_infos: {
      '11155111': {
        holders_count: '141268',
        total_supply: '943969542711126',
        is_verified: true,
      },
    },
  },
  token_id: null,
  token_instance: null,
  value: '2860471393',
  chain_values: {
    '11155111': '2860471393',
  },
};

export const HOMEPAGE_STATS: stats.MainPageMultichainStats = {
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
  yesterday_txns_multichain: {
    id: 'yesterdayTxnsMultichain',
    value: '1026175',
    title: 'Yesterday txns',
    units: undefined,
    description: 'Number of transactions yesterday (0:00 - 23:59 UTC) across all chains in the cluster',
  },
};

export const CHAIN_METRICS: multichain.ChainMetrics = {
  chain_id: '11155111',
  tps: '5.32',
  new_addresses: {
    current_full_week: '16450',
    previous_full_week: '156832',
    wow_diff_percent: '-89.51',
  },
  daily_transactions: {
    current_full_week: '3343480',
    previous_full_week: '4228309',
    wow_diff_percent: '-20.93',
  },
  active_accounts: {
    current_full_week: '124564',
    previous_full_week: '287092',
    wow_diff_percent: '-56.61',
  },
};
