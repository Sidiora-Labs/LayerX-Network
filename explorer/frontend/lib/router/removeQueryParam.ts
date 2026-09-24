import type { NextRouter } from 'next/router';

export default function removeQueryParam(router: NextRouter, param: string) {
  const { pathname, query, asPath } = router;
  const newQuery = { ...query };
  delete newQuery[param];

  const hashIndex = asPath.indexOf('#');
  const hash = hashIndex !== -1 ? asPath.substring(hashIndex) : '';

  router.replace({ pathname, query: newQuery, hash }, undefined, { shallow: true });
}
