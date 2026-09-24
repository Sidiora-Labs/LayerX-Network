import type * as multichain from '@blockscout/multichain-aggregator-types';

import { chainA, chainB, chainC } from './chains';

export const chainMetrics: multichain.ListChainMetricsResponse = {
  items: [
    {
      chain_id: chainA.id,
      active_accounts: {
        current_full_week: '1000',
        previous_full_week: '900',
        wow_diff_percent: '10',
      },
      tps: '1000',
      new_addresses: {
        current_full_week: '500',
        previous_full_week: '600',
        wow_diff_percent: '-20',
      },
    },
    {
      chain_id: chainB.id,
      active_accounts: undefined,
      tps: '42',
      new_addresses: {
        current_full_week: '200',
        previous_full_week: '10',
        wow_diff_percent: '900',
      },
    },
    {
      chain_id: chainC.id,
      active_accounts: {
        current_full_week: '100',
        previous_full_week: '100',
        wow_diff_percent: '0',
      },
    },
  ],
};
