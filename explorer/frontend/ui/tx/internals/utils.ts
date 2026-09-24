import type { TxInternalsType } from 'types/api/internalTransaction';

export type Sort = 'value-asc' | 'value-desc' | 'gas-limit-asc' | 'gas-limit-desc' | 'default';
export type SortField = 'value' | 'gas-limit';

interface TxInternalsTypeItem {
  title: string;
  id: TxInternalsType;
}

export const TX_INTERNALS_ITEMS: Array<TxInternalsTypeItem> = [
  { title: 'Call', id: 'call' },
  { title: 'Delegate call', id: 'delegatecall' },
  { title: 'Static call', id: 'staticcall' },
  { title: 'Create', id: 'create' },
  { title: 'Create2', id: 'create2' },
  { title: 'Self-destruct', id: 'selfdestruct' },
  { title: 'Reward', id: 'reward' },
];
