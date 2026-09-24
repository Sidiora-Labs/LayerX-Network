import type { ApiResource } from '../types';
import type * as rewards from '@blockscout/points-types';

export const REWARDS_API_RESOURCES = {
  config: {
    path: '/api/v1/config',
  },
  check_ref_code: {
    path: '/api/v1/auth/code/:code',
    pathParams: [ 'code' as const ],
  },
  nonce: {
    path: '/api/v1/auth/nonce',
  },
  check_user: {
    path: '/api/v1/auth/user/:address',
    pathParams: [ 'address' as const ],
  },
  login: {
    path: '/api/v1/auth/login',
  },
  logout: {
    path: '/api/v1/auth/logout',
  },
  user_balances: {
    path: '/api/v1/user/balances',
  },
  user_daily_check: {
    path: '/api/v1/user/daily/check',
  },
  user_daily_claim: {
    path: '/api/v1/user/daily/claim',
  },
  user_referrals: {
    path: '/api/v1/user/referrals',
  },
  user_badges: {
    path: '/api/v1/user/badges',
  },
  user_check_activity_pass: {
    path: '/api/v1/activity/check-pass',
    filterFields: [ 'address' as const ],
  },
  user_activity: {
    path: '/api/v1/user/activity/rewards',
  },
  user_activity_track_tx: {
    path: '/api/v1/user/activity/track/transaction',
  },
  user_activity_track_tx_confirm: {
    path: '/api/v1/activity/track/transaction/confirm',
  },
  user_activity_track_contract: {
    path: '/api/v1/user/activity/track/contract',
  },
  user_activity_track_contract_confirm: {
    path: '/api/v1/activity/track/contract/confirm',
  },
  user_activity_track_usage: {
    path: '/api/v1/user/activity/track/usage',
  },
  instances: {
    path: '/api/v1/instances',
  },
} satisfies Record<string, ApiResource>;

export type RewardsApiResourceName = `rewards:${ keyof typeof REWARDS_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type RewardsApiResourcePayload<R extends RewardsApiResourceName> =
R extends 'rewards:config' ? rewards.GetConfigResponse :
R extends 'rewards:check_ref_code' ? rewards.AuthCodeResponse :
R extends 'rewards:nonce' ? rewards.AuthNonceResponse :
R extends 'rewards:check_user' ? rewards.AuthUserResponse :
R extends 'rewards:login' ? rewards.AuthLoginResponse :
R extends 'rewards:user_balances' ? rewards.GetUserBalancesResponse :
R extends 'rewards:user_daily_check' ? rewards.DailyRewardCheckResponse :
R extends 'rewards:user_daily_claim' ? rewards.DailyRewardClaimResponse :
R extends 'rewards:user_referrals' ? rewards.GetReferralDataResponse :
R extends 'rewards:user_badges' ? rewards.GetAvailableBadgesResponse :
R extends 'rewards:user_check_activity_pass' ? rewards.CheckActivityPassResponse :
R extends 'rewards:user_activity' ? rewards.GetActivityRewardsResponse :
R extends 'rewards:user_activity_track_tx' ? rewards.PreSubmitTransactionResponse :
R extends 'rewards:user_activity_track_contract' ? rewards.PreVerifyContractResponse :
R extends 'rewards:instances' ? rewards.GetInstancesResponse :
never;
/* eslint-enable @stylistic/indent */
