import type { ApiResource } from '../../types';
import type { BlockCountdownResponse } from 'types/api/block';

export const GENERAL_API_V1_RESOURCES = {
  graphql: {
    path: '/api/v1/graphql',
  },
  block_countdown: {
    path: '/api',
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiV1ResourceName = `general:${ keyof typeof GENERAL_API_V1_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiV1ResourcePayload<R extends GeneralApiV1ResourceName> =
R extends 'general:block_countdown' ? BlockCountdownResponse :
never;
/* eslint-enable @stylistic/indent */
