import type { ApiResource } from '../types';
import type * as zetaChainCCTXType from '@blockscout/zetachain-cctx-types';
import type { ZetaChainCCTXFilterParams } from 'types/client/zetaChain';

export const ZETA_CHAIN_API_RESOURCES = {
  transactions: {
    path: '/api/v1/CctxInfo\\:list',
    filterFields: [
      'limit' as const,
      'offset' as const,
      'status_reduced' as const,
      'start_timestamp' as const,
      'end_timestamp' as const,
      'sender_address' as const,
      'receiver_address' as const,
      'source_chain_id' as const,
      'target_chain_id' as const,
      'token_symbol' as const,
      'coin_type' as const,
      'hash' as const,
      'age' as const, // frontend only
    ],
    paginated: true,
  },
  transaction: {
    path: '/api/v1/CctxInfo\\:get',
    filterFields: [ 'cctx_id' as const ],
  },
  tokens: {
    path: '/api/v1/TokenInfo\\:list',
  },
} satisfies Record<string, ApiResource>;

export type ZetaChainApiResourceName = `zetachain:${ keyof typeof ZETA_CHAIN_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type ZetaChainApiResourcePayload<R extends ZetaChainApiResourceName> =
R extends 'zetachain:transactions' ? zetaChainCCTXType.ListCctxsResponse :
R extends 'zetachain:transaction' ? zetaChainCCTXType.CrossChainTx :
R extends 'zetachain:tokens' ? zetaChainCCTXType.Tokens :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type ZetaChainApiPaginationFilters<R extends ZetaChainApiResourceName> =
R extends 'zetachain:transactions' ? ZetaChainCCTXFilterParams :
never;
/* eslint-enable @stylistic/indent */
