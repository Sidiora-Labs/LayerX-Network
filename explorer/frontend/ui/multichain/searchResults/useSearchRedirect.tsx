import type { UseQueryResult } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';

import multichainConfig from 'configs/multichain';
import type { ResourceError } from 'lib/api/resources';
import getQueryParamString from 'lib/router/getQueryParamString';
import removeQueryParam from 'lib/router/removeQueryParam';

interface Params {
  checkRedirectQuery: UseQueryResult<multichain.CheckRedirectResponse, ResourceError<unknown>>;
  hasSearchTerm: boolean;
}

export default function useSearchRedirect({ checkRedirectQuery, hasSearchTerm }: Params) {
  const router = useRouter();
  const enabled = getQueryParamString(router.query.redirect) === 'true';

  const [ showContent, setShowContent ] = React.useState(!enabled);

  React.useEffect(() => {
    if (showContent) {
      return;
    }

    if (!hasSearchTerm) {
      setShowContent(true);
      return;
    }

    if (checkRedirectQuery.data?.redirect && checkRedirectQuery.data.parameter) {
      switch (checkRedirectQuery.data.type) {
        case 'block': {
          const chainInfo = multichainConfig()?.chains.find((chain) => chain.id === checkRedirectQuery.data.chain_id);
          if (chainInfo) {
            router.replace({
              pathname: '/chain/[chain_slug]/block/[height_or_hash]',
              query: { height_or_hash: checkRedirectQuery.data.parameter, chain_slug: chainInfo?.slug },
            });
            return;
          }
          break;
        }
        case 'address': {
          router.replace({ pathname: '/address/[hash]', query: { hash: checkRedirectQuery.data.parameter } });
          return;
        }
        case 'transaction': {
          const chainInfo = multichainConfig()?.chains.find((chain) => chain.id === checkRedirectQuery.data.chain_id);
          if (chainInfo) {
            router.replace({
              pathname: '/chain/[chain_slug]/tx/[hash]',
              query: {
                hash: checkRedirectQuery.data.parameter,
                chain_slug: chainInfo?.slug,
              },
            });
            return;
          }
          break;
        }
      }
    }

    if (!checkRedirectQuery.isPending) {
      setShowContent(true);
      removeQueryParam(router, 'redirect');
    }
  }, [ checkRedirectQuery, router, hasSearchTerm, showContent ]);

  return showContent;
}
