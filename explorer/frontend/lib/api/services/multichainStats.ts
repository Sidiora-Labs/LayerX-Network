import type { ApiResource } from '../types';
import type * as stats from '@blockscout/stats-types';

export const MULTICHAIN_STATS_API_RESOURCES = {
  pages_main: {
    path: '/api/v1/pages/multichain/main',
  },
} satisfies Record<string, ApiResource>;

export type MultichainStatsApiResourceName = `multichainStats:${ keyof typeof MULTICHAIN_STATS_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type MultichainStatsApiResourcePayload<R extends MultichainStatsApiResourceName> =
R extends 'multichainStats:pages_main' ? stats.MainPageMultichainStats :
never;
/* eslint-enable @stylistic/indent */
