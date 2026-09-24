import type { VerifiedContractsSortingValue, VerifiedContractsSortingField } from 'types/api/verifiedContracts';

import type { SelectOption } from 'toolkit/chakra/select';

export const SORT_OPTIONS: Array<SelectOption<VerifiedContractsSortingValue>> = [
  { label: 'Default', value: 'default' },
  { label: 'Balance descending', value: 'balance-desc' },
  { label: 'Balance ascending', value: 'balance-asc' },
  { label: 'Txs count descending', value: 'transactions_count-desc' },
  { label: 'Txs count ascending', value: 'transactions_count-asc' },
];

export const SORT_SEQUENCE: Record<VerifiedContractsSortingField, Array<VerifiedContractsSortingValue>> = {
  balance: [ 'balance-desc', 'balance-asc', 'default' ],
  transactions_count: [ 'transactions_count-desc', 'transactions_count-asc', 'default' ],
};
