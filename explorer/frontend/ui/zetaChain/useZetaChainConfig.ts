import { useQuery } from '@tanstack/react-query';
import React from 'react';

import type { ZetaChainChainsConfigEnv, ZetaChainExternalChainConfig } from 'types/client/zetaChain';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import useFetch from 'lib/hooks/useFetch';

const zetachainFeature = config.features.zetachain;

export default function useZetaChainConfig() {

  const fetch = useFetch();
  const { isPending, data } = useQuery<Array<ZetaChainChainsConfigEnv>, ResourceError<unknown>, Array<ZetaChainExternalChainConfig>>({
    queryKey: [ 'zetachain-config' ],
    queryFn: async() => fetch(
      zetachainFeature.isEnabled ? zetachainFeature.chainsConfigUrl : '',
      undefined,
      { resource: 'zetachain-config' },
    ) as Promise<Array<ZetaChainChainsConfigEnv>>,
    select: (data) => {
      return data.map((item) => ({
        id: item.chain_id.toString(),
        name: item.chain_name || `Chain ${ item.chain_id }`,
        logo: item.chain_logo || undefined,
        explorer_url: item.instance_url,
        address_url_template: item.address_url_template,
        tx_url_template: item.tx_url_template,
      }));
    },
    enabled: Boolean(zetachainFeature.isEnabled),
    staleTime: Infinity,
  });

  return React.useMemo(() => ({
    isPending,
    data,
  }), [ data, isPending ]);
}
