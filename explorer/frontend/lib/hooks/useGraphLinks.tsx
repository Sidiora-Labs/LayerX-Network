import { useQuery } from '@tanstack/react-query';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import useFetch from 'lib/hooks/useFetch';

const feature = config.features.marketplace;

export default function useGraphLinks() {
  const fetch = useFetch();

  return useQuery<unknown, ResourceError<unknown>, Record<string, Array<{ title: string; url: string }>>>({
    queryKey: [ 'graph-links' ],
    queryFn: async() => fetch((feature.isEnabled && feature.graphLinksUrl) ? feature.graphLinksUrl : '', undefined, { resource: 'graph-links' }),
    enabled: feature.isEnabled && Boolean(feature.graphLinksUrl),
    staleTime: Infinity,
    placeholderData: {},
  });
}
