import type { AddressParam } from './addressParams';

export type FheOperationType =
  | 'arithmetic' |
  'bitwise' |
  'comparison' |
  'unary' |
  'control' |
  'encryption' |
  'random';

export type FheType =
  | 'Bool' |
  'Uint8' |
  'Uint16' |
  'Uint32' |
  'Uint64' |
  'Uint128' |
  'Uint160' |
  'Uint256' |
  'Bytes64' |
  'Bytes128' |
  'Bytes256';

export interface FheOperationInputs {
  lhs?: string;
  rhs?: string;
  ct?: string;
  control?: string;
  if_true?: string;
  if_false?: string;
  plaintext?: number;
}

export interface FheOperation {
  log_index: number;
  operation: string;
  type: FheOperationType;
  fhe_type: FheType;
  is_scalar: boolean;
  hcu_cost: number;
  hcu_depth: number;
  caller: AddressParam | null;
  inputs: FheOperationInputs;
  result: string;
  block_number: number;
}

export interface FheOperationsResponse {
  items: Array<FheOperation>;
  total_hcu: number;
  max_depth_hcu: number;
  operation_count: number;
}
