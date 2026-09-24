import type { NextRouter } from 'next/router';

export default function updateQueryParam(router: NextRouter, param: string, newValue: string) {
  const { pathname, query, asPath } = router;
  const newQuery = { ...query };
  newQuery[param] = newValue;

  const hashIndex = asPath.indexOf('#');
  const hash = hashIndex !== -1 ? asPath.substring(hashIndex) : '';

  router.replace({ pathname, query: newQuery, hash }, undefined, { shallow: true });
}
