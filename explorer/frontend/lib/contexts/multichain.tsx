import { useRouter } from 'next/router';
import React from 'react';

import type { ClusterChainConfig } from 'types/multichain';

import multichainConfig from 'configs/multichain';
import getChainIdFromSlug from 'lib/multichain/getChainIdFromSlug';
import getQueryParamString from 'lib/router/getQueryParamString';

interface MultichainProviderProps {
  children: React.ReactNode;
  chainId?: string;
}

export interface TMultichainContext {
  chain: ClusterChainConfig;
}

export const MultichainContext = React.createContext<TMultichainContext | null>(null);

export function MultichainProvider({ children, chainId: chainIdProp }: MultichainProviderProps) {
  const router = useRouter();
  const chainSlugQueryParam = router.pathname.includes('chain_slug') ? getQueryParamString(router.query.chain_slug) : undefined;
  const chainIdQueryParam = router.query.chain_id ? getQueryParamString(router.query.chain_id) : undefined;

  const [ chainId, setChainId ] = React.useState<string | undefined>(
    chainIdProp ??
    chainIdQueryParam ??
    (chainSlugQueryParam ? getChainIdFromSlug(chainSlugQueryParam) : undefined),
  );

  React.useEffect(() => {
    if (chainIdProp) {
      setChainId(chainIdProp);
    }
  }, [ chainIdProp ]);

  const chain = React.useMemo(() => {
    const config = multichainConfig();
    if (!config) {
      return;
    }

    if (!chainId) {
      return;
    }

    return config.chains.find((chain) => chain.id === chainId);
  }, [ chainId ]);

  const value = React.useMemo(() => {
    if (!chain) {
      return null;
    }

    return {
      chain,
    };
  }, [ chain ]);

  return (
    <MultichainContext.Provider value={ value }>
      { children }
    </MultichainContext.Provider>
  );
}

export function useMultichainContext(disabled: boolean = !multichainConfig) {
  const context = React.useContext(MultichainContext);
  if (context === undefined || disabled) {
    return null;
  }
  return context;
}
