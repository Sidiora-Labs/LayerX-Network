import type { ApiResource } from '../../types';
import type { PaxeerXCapabilities, PaxeerXUnifiedAccount } from 'types/api/paxeerX';

export const GENERAL_API_PAXEER_X_RESOURCES = {
  paxeer_x_unified_account: {
    path: '/api/v2/addresses/:hash/unified',
    pathParams: [ 'hash' as const ],
  },
  paxeer_x_capabilities: {
    path: '/api/v2/paxeer-x/capabilities',
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiPaxeerXResourceName = `general:${ keyof typeof GENERAL_API_PAXEER_X_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiPaxeerXResourcePayload<R extends GeneralApiPaxeerXResourceName> =
R extends 'general:paxeer_x_unified_account' ? PaxeerXUnifiedAccount :
R extends 'general:paxeer_x_capabilities' ? PaxeerXCapabilities :
never;
/* eslint-enable @stylistic/indent */
