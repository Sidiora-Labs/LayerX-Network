import { useRouter } from 'next/router';
import { useCallback } from 'react';

export function useQueryParams() {
  const router = useRouter();

  const updateQuery = useCallback((updates: Record<string, string | undefined>, replace = false) => {
    const newQuery = { ...router.query };

    Object.entries(updates).forEach(([ key, value ]) => {
      if (value === undefined) {
        delete newQuery[key];
      } else {
        newQuery[key] = value;
      }
    });

    const routerFn = replace ? router.replace : router.push;
    routerFn({ pathname: router.pathname, query: newQuery }, undefined, { shallow: true });
  }, [ router ]);

  return { updateQuery };
}
