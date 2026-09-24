import type { AddressFromToFilter } from 'types/api/address';

export type CsvExportParams = {
  type: 'transactions' | 'internal-transactions' | 'token-transfers';
  filterType?: 'address';
  filterValue?: AddressFromToFilter;
} | {
  type: 'logs';
  filterType?: 'topic';
  filterValue?: string;
} | {
  type: 'holders';
  filterType?: undefined;
  filterValue?: undefined;
} | {
  type: 'epoch-rewards';
  filterType?: undefined;
  filterValue?: undefined;
};
