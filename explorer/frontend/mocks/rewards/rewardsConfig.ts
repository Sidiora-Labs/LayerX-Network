import type { GetConfigResponse } from '@blockscout/points-types';

export const base: GetConfigResponse = {
  rewards: {
    registration: '100',
    registration_with_referral: '200',
    daily_claim: '10',
    referral_share: '0.1',
    streak_bonuses: {},
    sent_transactions_activity_rewards: {
      '1': '100',
    },
    verified_contracts_activity_rewards: {
      '1': '100',
    },
    blockscout_usage_activity_rewards: {
      '1': '100',
    },
    blockscout_activity_pass_id: '1',
  },
  auth: {
    shared_siwe_login: true,
  },
  activity: {
    sent_transactions_activity_enabled: true,
    verified_contracts_activity_enabled: true,
    blockscout_usage_activity_enabled: true,
  },
};
