import type { EnsLookupSorting } from 'types/api/ens';

import type { SelectOption } from 'toolkit/chakra/select';
import getNextSortValueShared from 'ui/shared/sort/getNextSortValue';

export type SortField = EnsLookupSorting['sort'];
export type Sort = `${ EnsLookupSorting['sort'] }-${ EnsLookupSorting['order'] }` | 'default';

export const SORT_OPTIONS: Array<SelectOption<Sort>> = [
  { label: 'Default', value: 'default' },
  { label: 'Registered on descending', value: 'registration_date-DESC' },
  { label: 'Registered on ascending', value: 'registration_date-ASC' },
];

const SORT_SEQUENCE: Record<SortField, Array<Sort>> = {
  registration_date: [ 'registration_date-DESC', 'registration_date-ASC', 'default' ],
};

export const getNextSortValue = (getNextSortValueShared<SortField, Sort>).bind(undefined, SORT_SEQUENCE);
