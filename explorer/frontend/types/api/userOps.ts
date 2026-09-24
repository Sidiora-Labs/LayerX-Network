import type { AddressParamBasic } from './addressParams';
import type { DecodedInput } from './decodedInput';

export type UserOpsItem = {
  hash: string;
  block_number: string;
  transaction_hash: string;
  address: string | AddressParamBasic;
  timestamp: string;
  status: boolean;
  fee: string;
};

export type UserOpsResponse = {
  items: Array<UserOpsItem>;
  next_page_params: {
    page_token: string;
    page_size: number;
  } | null;
};

export type UserOpSponsorType = 'paymaster_hybrid' | 'paymaster_sponsor' | 'wallet_balance' | 'wallet_deposit';

export type UserOp = {
  hash: string;
  sender: string | AddressParamBasic;
  status: boolean;
  revert_reason: string | null;
  timestamp: string | null;
  fee: string;
  gas: string;
  transaction_hash: string;
  block_number: string;
  block_hash: string;
  entry_point: string | AddressParamBasic;
  call_gas_limit: string;
  verification_gas_limit: string;
  pre_verification_gas: string;
  max_fee_per_gas: string;
  max_priority_fee_per_gas: string;
  aggregator: string | null;
  aggregator_signature: string | null;
  bundler: string | AddressParamBasic;
  factory: string | null;
  paymaster: string | AddressParamBasic | null;
  sponsor_type: UserOpSponsorType;
  signature: string;
  nonce: string;
  call_data: string;
  decoded_call_data: DecodedInput | null;
  execute_call_data: string | null;
  execute_target?: AddressParamBasic | null;
  decoded_execute_call_data: DecodedInput | null;
  user_logs_start_index: number;
  user_logs_count: number;
  raw: {
    account_gas_limits?: string;
    call_data: string;
    call_gas_limit: string;
    gas_fees?: string;
    init_code: string;
    max_fee_per_gas: string;
    max_priority_fee_per_gas: string;
    nonce: string;
    paymaster_and_data: string;
    pre_verification_gas: string;
    sender: string;
    signature: string;
    verification_gas_limit: string;
  };
  gas_price: string;
  gas_used: string;
};

export type UserOpsFilters = {
  transaction_hash?: string;
  sender?: string;
};

export type UserOpsAccount = {
  total_ops: number;
};
