import type { GetActivityRewardsResponse } from '@blockscout/points-types';

export const base: GetActivityRewardsResponse = {
  items: [
    {
      date: '2025-03-10',
      end_date: '2025-03-16',
      activity: 'sent_transactions',
      amount: '60',
      percentile: 0.5,
      is_pending: true,
    },
    {
      date: '2025-03-10',
      end_date: '2025-03-16',
      activity: 'verified_contracts',
      amount: '40',
      percentile: 0.3,
      is_pending: true,
    },
    {
      date: '2025-03-10',
      end_date: '2025-03-16',
      activity: 'blockscout_usage',
      amount: '80',
      percentile: 0.8,
      is_pending: true,
    },
  ],
  last_week: [
    {
      date: '2025-03-03',
      end_date: '2025-03-09',
      activity: 'sent_transactions',
      amount: '40',
      percentile: 0.25,
      is_pending: false,
    },
    {
      date: '2025-03-03',
      end_date: '2025-03-09',
      activity: 'verified_contracts',
      amount: '60',
      percentile: 0.6,
      is_pending: false,
    },
    {
      date: '2025-03-03',
      end_date: '2025-03-09',
      activity: 'blockscout_usage',
      amount: '100',
      percentile: 0.95,
      is_pending: false,
    },
  ],
};
