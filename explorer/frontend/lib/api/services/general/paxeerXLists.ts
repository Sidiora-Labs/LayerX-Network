import type { ApiResource } from '../../types';
import type { PaxeerXAnchorsResponse, PaxeerXReceiptsResponse, PaxeerXTxStatus } from 'types/api/paxeerXLists';

export const GENERAL_API_PAXEER_X_LISTS_RESOURCES = {
  paxeer_x_tx_status: {
    path: '/api/v2/transactions/:hash/status',
    pathParams: [ 'hash' as const ],
  },
  paxeer_x_anchors: {
    path: '/api/v2/paxeer-x/anchors',
    filterFields: [],
    paginated: true,
  },
  paxeer_x_receipts: {
    path: '/api/v2/paxeer-x/receipts',
    filterFields: [],
    paginated: true,
  },
} satisfies Record<string, ApiResource>;

export type GeneralApiPaxeerXListsResourceName = `general:${ keyof typeof GENERAL_API_PAXEER_X_LISTS_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type GeneralApiPaxeerXListsResourcePayload<R extends GeneralApiPaxeerXListsResourceName> =
R extends 'general:paxeer_x_tx_status' ? PaxeerXTxStatus :
R extends 'general:paxeer_x_anchors' ? PaxeerXAnchorsResponse :
R extends 'general:paxeer_x_receipts' ? PaxeerXReceiptsResponse :
never;
/* eslint-enable @stylistic/indent */
