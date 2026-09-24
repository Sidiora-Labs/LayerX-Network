import type { Transaction, TransactionsSortingValue } from 'types/api/transaction';

import compareBns from 'lib/bigint/compareBns';

export default function sortTxs(sorting: TransactionsSortingValue | undefined) {
  return function sortingFn(tx1: Transaction, tx2: Transaction) {
    switch (sorting) {
      case 'value-desc':
        return compareBns(tx2.value, tx1.value);
      case 'value-asc':
        return compareBns(tx1.value, tx2.value);
      case 'fee-desc':
        return compareBns(tx2.fee.value || 0, tx1.fee.value || 0);
      case 'fee-asc':
        return compareBns(tx1.fee.value || 0, tx2.fee.value || 0);
      case 'block_number-asc': {
        if (tx1.block_number && tx2.block_number) {
          return tx1.block_number - tx2.block_number;
        }
        return 0;
      }
      default:
        return 0;
    }
  };
}

export function sortTxsFromSocket(sorting: TransactionsSortingValue | undefined) {
  if (sorting && sorting !== 'default') {
    return sortTxs(sorting);
  }

  return function sortingFn(tx1: Transaction, tx2: Transaction) {
    if (!tx1.timestamp) {
      return -1;
    }

    if (!tx2.timestamp) {
      return 1;
    }

    return tx2.timestamp.localeCompare(tx1.timestamp);
  };
}
