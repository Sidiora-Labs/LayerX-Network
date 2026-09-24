import type * as bens from '@blockscout/bens-types';

import getNextSortValueShared from 'ui/shared/sort/getNextSortValue';

export type SortField = 'timestamp';
export type Sort = `${ SortField }-asc` | `${ SortField }-desc` | 'default';

const SORT_SEQUENCE: Record<SortField, Array<Sort>> = {
  timestamp: [ 'timestamp-desc', 'timestamp-asc', 'default' ],
};

export const getNextSortValue = (getNextSortValueShared<SortField, Sort>).bind(undefined, SORT_SEQUENCE);

export const sortFn = (sort: Sort | undefined) => (a: bens.DomainEvent, b: bens.DomainEvent) => {
  switch (sort) {
    case 'timestamp-asc': {
      return b.timestamp.localeCompare(a.timestamp);
    }

    case 'timestamp-desc': {
      return a.timestamp.localeCompare(b.timestamp);
    }

    default:
      return 0;
  }
};
