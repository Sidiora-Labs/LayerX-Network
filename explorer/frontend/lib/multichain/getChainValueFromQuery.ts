import type { Router } from 'next/router';

import multichainConfig from 'configs/multichain';
import getQueryParamString from 'lib/router/getQueryParamString';

export default function getChainValueFromQuery(
  query: Router['query'],
  chainIds?: Array<string>,
  withAllOption?: boolean,
) {
  const config = multichainConfig();
  if (!config) {
    return undefined;
  }

  const chainId = getQueryParamString(query.chain_id);
  const chainSlug = getQueryParamString(query.chain_slug);

  if (chainId) {
    if (config.chains.some((chain) => chain.id === chainId)) {
      return chainId;
    }
  }

  if (chainSlug) {
    const chain = config.chains.find((chain) => chain.slug === chainSlug);
    if (chain) {
      return chain.id;
    }
  }

  if (withAllOption) {
    return 'all';
  }

  return config.chains.filter((chain) => !chainIds || (chain.id && chainIds.includes(chain.id)))?.[0]?.id;
}
