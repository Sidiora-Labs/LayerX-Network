import { useRouter } from 'next/router';
import React from 'react';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';

const DOMAIN_NAME_REGEXP = /.\../;

const feature = config.features.nameServices;

export default function useCheckDomainNameParam(hashOrDomainName: string) {
  const router = useRouter();
  const maybeDomainName = DOMAIN_NAME_REGEXP.test(hashOrDomainName);
  const isQueryEnabled = feature.isEnabled && feature.ens.isEnabled && maybeDomainName;
  const [ isLoading, setIsLoading ] = React.useState(isQueryEnabled);

  const domainLookupQuery = useApiQuery('bens:domains_lookup', {
    queryParams: {
      name: hashOrDomainName,
      protocols: feature.isEnabled && feature.ens.isEnabled ? feature.ens.protocols : undefined,
      only_active: false,
    },
    queryOptions: {
      enabled: isQueryEnabled,
    },
  });

  React.useEffect(() => {
    if (domainLookupQuery.isPending) {
      return;
    }

    const firstDomainAddress = domainLookupQuery.data?.items[0]?.resolved_address?.hash;
    if (firstDomainAddress) {
      router.replace({ pathname: '/address/[hash]', query: { hash: firstDomainAddress } });
    } else {
      setIsLoading(false);
    }
  }, [ domainLookupQuery.isPending, domainLookupQuery.data, router ]);

  React.useEffect(() => {
    if (!maybeDomainName) {
      setIsLoading(false);
    }
  }, [ maybeDomainName ]);

  return isLoading;
}
