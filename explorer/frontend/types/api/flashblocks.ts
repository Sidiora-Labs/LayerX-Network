export interface FlashblockItemApiOptimism {
  payload_id: string;
  index: number;
  base?: {
    parent_beacon_block_root: string;
    parent_hash: string;
    fee_recipient: string;
    prev_randao: string;
    block_number: string;
    gas_limit: string;
    timestamp: string;
    extra_data: string;
    base_fee_per_gas: string;
  };
  diff: {
    state_root: string;
    receipts_root: string;
    logs_bloom: string;
    gas_used: string;
    block_hash: string;
    transactions: Array<string>;
  };
  metadata: {
    receipts: Record<string, unknown>;
  };
}

export interface FlashblockItemApiMegaEth {
  block_number: string;
  block_timestamp: string;
  gas_used: string;
  index: string;
  mini_block_number: number;
  mini_block_timestamp: string;
  receipts: Array<unknown>;
  transactions: Array<unknown>;
}
