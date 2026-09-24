export interface VerifiedContractsSorting {
  sort: 'balance' | 'transactions_count';
  order: 'asc' | 'desc';
}

export type VerifiedContractsSortingField = VerifiedContractsSorting['sort'];

export type VerifiedContractsSortingValue = `${ VerifiedContractsSortingField }-${ VerifiedContractsSorting['order'] }` | 'default';
