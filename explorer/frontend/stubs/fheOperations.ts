import type { FheOperation, FheOperationsResponse } from 'types/api/fheOperations';

import { ADDRESS_PARAMS } from './addressParams';

export const FHE_OPERATION: FheOperation = {
  log_index: 0,
  operation: 'fhe_add',
  type: 'arithmetic',
  fhe_type: 'Uint64',
  is_scalar: true,
  hcu_cost: 100,
  hcu_depth: 50,
  caller: ADDRESS_PARAMS,
  inputs: {},
  result: '0x',
  block_number: 9004925,
};

export const FHE_OPERATIONS_RESPONSE: FheOperationsResponse = {
  items: [
    { ...FHE_OPERATION, log_index: 0 },
    { ...FHE_OPERATION, log_index: 1 },
    { ...FHE_OPERATION, log_index: 2 },
  ],
  total_hcu: 300,
  max_depth_hcu: 100,
  operation_count: 3,
};
