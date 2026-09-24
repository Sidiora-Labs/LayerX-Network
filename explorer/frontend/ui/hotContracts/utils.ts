import type { HotContractsSortingValue, HotContractsSortingField, HotContractsInterval } from 'types/api/contracts';

import getQueryParamString from 'lib/router/getQueryParamString';
import type { SelectOption } from 'toolkit/chakra/select';

export const SORT_OPTIONS: Array<SelectOption<HotContractsSortingValue>> = [
  { label: 'Default', value: 'default' },
  { label: 'Txs count descending', value: 'transactions_count-desc' },
  { label: 'Txs count ascending', value: 'transactions_count-asc' },
  { label: 'Gas used descending', value: 'total_gas_used-desc' },
  { label: 'Gas used ascending', value: 'total_gas_used-asc' },
];

export const SORT_SEQUENCE: Record<HotContractsSortingField, Array<HotContractsSortingValue>> = {
  transactions_count: [ 'transactions_count-desc', 'transactions_count-asc', 'default' ],
  total_gas_used: [ 'total_gas_used-desc', 'total_gas_used-asc', 'default' ],
};

export const INTERVAL_ITEMS: Array<{ id: HotContractsInterval; labelShort: string; labelFull: string }> = [
  { id: '5m', labelShort: '5m', labelFull: '5 minutes' },
  { id: '1h', labelShort: '1h', labelFull: '1 hour' },
  { id: '3h', labelShort: '3h', labelFull: '3 hours' },
  { id: '1d', labelShort: '1D', labelFull: '1 day' },
  { id: '7d', labelShort: '1W', labelFull: '1 week' },
  { id: '30d', labelShort: '1M', labelFull: '1 month' },
];

export const getIntervalValueFromQuery = (query: string | Array<string> | undefined): HotContractsInterval => {
  const queryString = getQueryParamString(query);
  if (queryString) {
    const interval = INTERVAL_ITEMS.find(item => item.id === queryString);
    if (interval) {
      return interval.id;
    }
  }

  return '1d';
};
