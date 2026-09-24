import type { Query } from 'nextjs-routes';

import getQueryParamString from 'lib/router/getQueryParamString';

export default function getSortParamsFromQuery<T>(query: Query, sortOptions: Record<string, Array<string | undefined>>) {
  if (!query.sort || !query.order) {
    return undefined;
  }

  const sortStr = getQueryParamString(query.sort);

  if (!Object.keys(sortOptions).includes(sortStr)) {
    return undefined;
  }

  const orderStr = getQueryParamString(query.order);

  if (!sortOptions[sortStr].includes(orderStr)) {
    return undefined;
  }

  return ({ sort: sortStr, order: orderStr } as T);
}
