import type { AddressParam } from './addressParams';

export type DepositStatus = 'pending' | 'invalid' | 'completed';

export type DepositsResponse = {
  items: Array<DepositsItem>;
  next_page_params: {
    index: number;
    items_count: number;
  } | null;
};

export type DepositsItem = {
  amount: string;
  block_number: number;
  block_hash: string;
  block_timestamp: string;
  index: number;
  pubkey: string;
  signature: string;
  status: DepositStatus;
  from_address: AddressParam;
  transaction_hash: string;
  withdrawal_address: AddressParam;
};

export type DepositsCounters = {
  deposits_count: string;
};
