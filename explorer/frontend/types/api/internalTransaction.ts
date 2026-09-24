import type { AddressParam } from './addressParams';

export type TxInternalsType = 'call' | 'delegatecall' | 'staticcall' | 'create' | 'create2' | 'selfdestruct' | 'reward';

export type InternalTransaction = (
  {
    to: AddressParam;
    created_contract: null;
  } |
  {
    to: null;
    created_contract: AddressParam;
  }
) & {
  error: string | null;
  success: boolean;
  type: TxInternalsType;
  transaction_hash: string;
  from: AddressParam;
  value: string;
  index: number;
  block_number: number;
  timestamp: string;
  gas_limit: string;
};

export interface InternalTransactionsResponse {
  items: Array<InternalTransaction>;
  next_page_params: {
    block_number: number;
    index: number;
    items_count: number;
    transaction_hash: string;
    transaction_index: number;
  } | null;
}

export interface InternalTransactionFilters {
  transaction_hash: string;
}
