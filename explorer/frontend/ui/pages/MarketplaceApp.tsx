import { Flex } from '@chakra-ui/react';
import { useQuery } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React, { useEffect, useMemo } from 'react';

import type { MarketplaceApp } from 'types/client/marketplace';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import useApiFetch from 'lib/api/useApiFetch';
import { useMarketplaceContext } from 'lib/contexts/marketplace';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import useFetch from 'lib/hooks/useFetch';
import * as metadata from 'lib/metadata';
import getQueryParamString from 'lib/router/getQueryParamString';
import { useColorMode } from 'toolkit/chakra/color-mode';
import useIsAuth from 'ui/snippets/auth/useIsAuth';

import MarketplaceAppIframe from '../marketplace/MarketplaceAppIframe';
import MarketplaceAppTopBar from '../marketplace/MarketplaceAppTopBar';
import useAutoConnectWallet from '../marketplace/useAutoConnectWallet';
import { getAppUrl } from '../marketplace/utils';

const feature = config.features.marketplace;

export default function MarketplaceApp() {
  const fetch = useFetch();
  const apiFetch = useApiFetch();
  const router = useRouter();
  const id = getQueryParamString(router.query.id);
  const isAuth = useIsAuth();
  const { colorMode } = useColorMode();
  useAutoConnectWallet();

  const query = useQuery<unknown, ResourceError<unknown>, MarketplaceApp>({
    queryKey: [ 'marketplace-dapps', id ],
    queryFn: async() => {
      if (!feature.isEnabled) {
        return null;
      } else if ('configUrl' in feature) {
        const result = await fetch<Array<MarketplaceApp>, unknown>(feature.configUrl, undefined, { resource: 'marketplace-dapps' });
        if (!Array.isArray(result)) {
          throw result;
        }
        const item = result.find((app: MarketplaceApp) => app.id === id);
        if (!item) {
          throw { status: 404 };
        }
        return item;
      } else {
        return apiFetch('admin:marketplace_dapp', { pathParams: { chainId: config.chain.id, dappId: id } });
      }
    },
    enabled: feature.isEnabled,
  });
  const { data, isPending, refetch } = query;

  React.useEffect(() => {
    refetch();
  }, [ isAuth, refetch ]);

  const { setIsAutoConnectDisabled } = useMarketplaceContext();

  const appUrl = useMemo(() => getAppUrl(data?.url, router), [ data?.url, router ]);

  const message = useMemo(() => ({
    blockscoutColorMode: colorMode,
    blockscoutRootUrl: config.app.baseUrl + route({ pathname: '/' }),
    blockscoutAddressExplorerUrl: config.app.baseUrl + route({ pathname: '/address/[hash]', query: { hash: '' } }),
    blockscoutTransactionExplorerUrl: config.app.baseUrl + route({ pathname: '/tx/[hash]', query: { hash: '' } }),
    blockscoutNetworkName: config.chain.name,
    blockscoutNetworkId: Number(config.chain.id),
    blockscoutNetworkCurrency: config.chain.currency,
    blockscoutNetworkRpc: config.chain.rpcUrls[0],
  }), [ colorMode ]);

  useEffect(() => {
    if (data) {
      metadata.update(
        { pathname: '/apps/[id]', query: { id: data.id } },
        { app_name: data.title },
      );
      setIsAutoConnectDisabled(!data.internalWallet);
    }
  }, [ data, setIsAutoConnectDisabled ]);

  throwOnResourceLoadError(query);

  return (
    <Flex flexDirection="column" h="100%">
      <MarketplaceAppTopBar
        appId={ id }
        data={ data }
        isLoading={ isPending }
      />
      <MarketplaceAppIframe
        appId={ id }
        appUrl={ appUrl }
        message={ message }
        mx={{ base: -4, lg: -6 }}
      />
    </Flex>
  );
};
