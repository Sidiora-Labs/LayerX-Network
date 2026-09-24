import type { AddressParam } from './addressParams';
import type { DecodedInput } from './decodedInput';

export interface Log {
  address: AddressParam;
  topics: Array<string | null>;
  data: string;
  index: number;
  decoded: DecodedInput | null;
  transaction_hash: string | null;
  block_timestamp: string | null;
}

export interface LogsResponseTx {
  items: Array<Log>;
  next_page_params: {
    index: number;
    items_count: number;
    transaction_hash: string;
  } | null;
}

export interface LogsResponseAddress {
  items: Array<Log>;
  next_page_params: {
    index: number;
    items_count: number;
    transaction_index: number;
    block_number: number;
  } | null;
}
