import type { ApiResource } from '../types';
import type * as tac from '@blockscout/tac-operation-lifecycle-types';

export const TAC_OPERATION_LIFECYCLE_API_RESOURCES = {
  operations: {
    path: '/api/v1/tac/operations',
    paginated: true,
    filterFields: [ 'q' ],
  },
  operation: {
    path: '/api/v1/tac/operations/:id',
    pathParams: [ 'id' ],
  },
  operation_by_tx_hash: {
    path: '/api/v1/tac/operations\\:byTx/:tx_hash',
    pathParams: [ 'tx_hash' ],
  },
  stat_operations: {
    path: '/api/v1/stat/operations',
  },
} satisfies Record<string, ApiResource>;

export type TacOperationLifecycleApiResourceName = `tac:${ keyof typeof TAC_OPERATION_LIFECYCLE_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type TacOperationLifecycleApiResourcePayload<R extends TacOperationLifecycleApiResourceName> =
R extends 'tac:operations' ? tac.OperationsResponse :
R extends 'tac:operation' ? tac.OperationDetails :
R extends 'tac:operation_by_tx_hash' ? tac.OperationsFullResponse :
R extends 'tac:stat_operations' ? tac.GetOperationStatisticsResponse :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type TacOperationLifecycleApiPaginationFilters<R extends TacOperationLifecycleApiResourceName> =
R extends 'tac:operations' ? { q: string } :
never;
/* eslint-enable @stylistic/indent */
