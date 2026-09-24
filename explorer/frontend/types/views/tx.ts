import type { ArrayElement } from 'types/utils';

export const TX_FIELDS_IDS = [
  'value',
  'fee_currency',
  'gas_price',
  'tx_fee',
  'gas_fees',
  'burnt_fees',
  'L1_status',
  'batch',
] as const;

export type TxFieldsId = ArrayElement<typeof TX_FIELDS_IDS>;

export const TX_ADDITIONAL_FIELDS_IDS = [
  'fee_per_gas',
  'set_max_gas_limit',
] as const;

export type TxAdditionalFieldsId = ArrayElement<typeof TX_ADDITIONAL_FIELDS_IDS>;
