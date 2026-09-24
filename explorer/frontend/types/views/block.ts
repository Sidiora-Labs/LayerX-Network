import type { ArrayElement } from 'types/utils';

export const BLOCK_FIELDS_IDS = [
  'base_fee',
  'burnt_fees',
  'total_reward',
  'nonce',
  'miner',
  'L1_status',
  'batch',
] as const;

export type BlockFieldId = ArrayElement<typeof BLOCK_FIELDS_IDS>;
