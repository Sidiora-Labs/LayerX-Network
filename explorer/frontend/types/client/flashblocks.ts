export interface FlashblockItem {
  block_number: number | undefined;
  index: number;
  transactions_count: number;
  gas_used: number;
  timestamp?: string;
}
