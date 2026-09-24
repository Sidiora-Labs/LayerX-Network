import { useQuery } from '@tanstack/react-query';
import React from 'react';

import type { MarketplaceApp } from 'types/client/marketplace';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import useApiFetch from 'lib/hooks/useFetch';
import { CATEGORIES } from 'stubs/marketplace';

const feature = config.features.marketplace;
const categoriesUrl = (feature.isEnabled && feature.categoriesUrl) || '';

export default function useMarketplaceCategories(apps: Array<MarketplaceApp> | undefined, isAppsPlaceholderData: boolean) {
  const apiFetch = useApiFetch();

  const { isPlaceholderData, data } = useQuery<unknown, ResourceError<unknown>, Array<string>>({
    queryKey: [ 'marketplace-categories' ],
    queryFn: async() => apiFetch(categoriesUrl, undefined, { resource: 'marketplace-categories' }),
    placeholderData: categoriesUrl ? CATEGORIES : undefined,
    staleTime: Infinity,
    enabled: Boolean(categoriesUrl),
  });

  const categories = React.useMemo(() => {
    if (isAppsPlaceholderData || isPlaceholderData) {
      return CATEGORIES.map(category => ({ name: category, count: 0 }));
    }

    let categoryNames: Array<string> = [];
    const grouped: { [key: string]: number } = {};

    apps?.forEach(app => {
      app.categories.forEach(category => {
        if (grouped[category] === undefined) {
          grouped[category] = 0;
        }
        grouped[category]++;
      });
    });

    if (data?.length && !isPlaceholderData) {
      categoryNames = data;
    } else {
      categoryNames = Object.keys(grouped);
    }

    return categoryNames
      .map(category => ({ name: category, count: grouped[category] || 0 }))
      .filter(c => c.count > 0);
  }, [ apps, isAppsPlaceholderData, data, isPlaceholderData ]);

  return React.useMemo(() => ({
    isPlaceholderData: isAppsPlaceholderData || isPlaceholderData,
    data: categories,
  }), [ isPlaceholderData, isAppsPlaceholderData, categories ]);
}
