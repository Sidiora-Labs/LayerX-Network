import { useQueries, useQueryClient } from '@tanstack/react-query';
import React from 'react';

import type { SocketMessage } from 'lib/socket/types';
import type { AddressTokenBalance, AddressTokensBalancesSocketMessage, AddressTokensResponse } from 'types/api/address';
import type { TokenType } from 'types/api/token';

import config from 'configs/app';
import useApiFetch from 'lib/api/useApiFetch';
import useApiQuery, { getResourceKey } from 'lib/api/useApiQuery';
import { useMultichainContext } from 'lib/contexts/multichain';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';

import type { TokenEnhancedData } from './tokenUtils';
import { calculateUsdValue } from './tokenUtils';

interface Props {
  hash?: string;
  enabled?: boolean;
}

const tokenBalanceItemIdentityFactory = (match: AddressTokenBalance) => (item: AddressTokenBalance) => ((
  match.token.address_hash === item.token.address_hash &&
  match.token_id === item.token_id &&
  match.token_instance?.id === item.token_instance?.id
));

const socketEventForTokenType = (tokenTypeId: string): string => {
  const normalizedTypeId = tokenTypeId.toLowerCase().replace(/[^a-z0-9]+/g, '_');
  return `updated_token_balances_${ normalizedTypeId }`;
};

const additionalTokenTypes = config.chain.additionalTokenTypes;

export default function useFetchTokens({ hash, enabled }: Props) {
  const erc20query = useApiQuery('general:address_tokens', {
    pathParams: { hash },
    queryParams: { type: 'ERC-20' },
    queryOptions: { enabled: Boolean(hash) && enabled, refetchOnMount: false },
  });
  const erc721query = useApiQuery('general:address_tokens', {
    pathParams: { hash },
    queryParams: { type: 'ERC-721' },
    queryOptions: { enabled: Boolean(hash) && enabled, refetchOnMount: false },
  });
  const erc1155query = useApiQuery('general:address_tokens', {
    pathParams: { hash },
    queryParams: { type: 'ERC-1155' },
    queryOptions: { enabled: Boolean(hash) && enabled, refetchOnMount: false },
  });
  const erc404query = useApiQuery('general:address_tokens', {
    pathParams: { hash },
    queryParams: { type: 'ERC-404' },
    queryOptions: { enabled: Boolean(hash) && enabled, refetchOnMount: false },
  });
  const apiFetch = useApiFetch();
  const multichainContext = useMultichainContext();
  const chain = multichainContext?.chain;

  const additionalTokenQueries = useQueries({
    queries: additionalTokenTypes.map((item) => ({
      queryKey: getResourceKey('general:address_tokens', { pathParams: { hash }, queryParams: { type: item.id as unknown as TokenType }, chainId: chain?.id }),
      queryFn: async({ signal }) => {
        return apiFetch('general:address_tokens', {
          pathParams: { hash },
          queryParams: { type: item.id as unknown as TokenType },
          chain,
          fetchParams: { signal },
        }) as Promise<AddressTokensResponse>;
      },
      enabled: Boolean(hash) && enabled,
      refetchOnMount: false,
    })),
  });

  const queryClient = useQueryClient();

  const updateTokensData = React.useCallback((type: TokenType | Array<TokenType>, payload: AddressTokensBalancesSocketMessage) => {
    const queryKey = getResourceKey('general:address_tokens', { pathParams: { hash }, queryParams: { type } });

    queryClient.setQueryData(queryKey, (prevData: AddressTokensResponse | undefined) => {
      const items = prevData?.items.map((currentItem) => {
        const updatedData = payload.token_balances.find(tokenBalanceItemIdentityFactory(currentItem));
        return updatedData ?? currentItem;
      }) || [];

      const extraItems = prevData?.next_page_params ?
        [] :
        payload.token_balances.filter((socketItem) => !items.some(tokenBalanceItemIdentityFactory(socketItem)));

      if (!prevData) {
        return {
          items: extraItems,
          next_page_params: null,
        };
      }

      return {
        items: items.concat(extraItems),
        next_page_params: prevData.next_page_params,
      };
    });
  }, [ hash, queryClient ]);

  const additionalTokenTypesIds = React.useMemo(() => {
    return additionalTokenTypes.map((item) => item.id);
  }, []);

  const handleTokenBalancesErc20Message: SocketMessage.AddressTokenBalancesErc20['handler'] = React.useCallback((payload) => {
    updateTokensData('ERC-20', payload);
    // update ERC-20 & additional token types query, that is used on the address tokens list
    updateTokensData([ 'ERC-20', ...additionalTokenTypesIds ], payload);
  }, [ updateTokensData, additionalTokenTypesIds ]);

  const handleTokenBalancesErc721Message: SocketMessage.AddressTokenBalancesErc721['handler'] = React.useCallback((payload) => {
    updateTokensData('ERC-721', payload);
  }, [ updateTokensData ]);

  const handleTokenBalancesErc1155Message: SocketMessage.AddressTokenBalancesErc1155['handler'] = React.useCallback((payload) => {
    updateTokensData('ERC-1155', payload);
  }, [ updateTokensData ]);

  const handleTokenBalancesErc404Message: SocketMessage.AddressTokenBalancesErc404['handler'] = React.useCallback((payload) => {
    updateTokensData('ERC-404', payload);
  }, [ updateTokensData ]);

  const channel = useSocketChannel({
    topic: `addresses:${ hash?.toLowerCase() }`,
    isDisabled:
      Boolean(hash) &&
      (erc20query.isPlaceholderData ||
        erc721query.isPlaceholderData ||
        erc1155query.isPlaceholderData ||
        erc404query.isPlaceholderData),
  });

  useSocketMessage({
    channel,
    event: 'updated_token_balances_erc_20',
    handler: handleTokenBalancesErc20Message,
  });
  useSocketMessage({
    channel,
    event: 'updated_token_balances_erc_721',
    handler: handleTokenBalancesErc721Message,
  });
  useSocketMessage({
    channel,
    event: 'updated_token_balances_erc_1155',
    handler: handleTokenBalancesErc1155Message,
  });
  useSocketMessage({
    channel,
    event: 'updated_token_balances_erc_404',
    handler: handleTokenBalancesErc404Message,
  });
  React.useEffect(() => {
    if (!channel || additionalTokenTypes.length === 0) {
      return;
    }

    const refs = additionalTokenTypes.map((item) => {
      const event = socketEventForTokenType(item.id);

      return channel.on(event, (payload: AddressTokensBalancesSocketMessage) => {
        updateTokensData(item.id as unknown as TokenType, payload);

        // keep the "ERC-20 & custom token types" list in sync
        updateTokensData([ 'ERC-20', ...additionalTokenTypesIds ], payload);
      });
    });

    return () => {
      refs.forEach((ref, index) => {
        channel.off(socketEventForTokenType(additionalTokenTypes[index]?.id || ''), ref);
      });
    };
  }, [ channel, additionalTokenTypesIds, updateTokensData ]);

  const data = React.useMemo(() => {
    const additionalGroups = additionalTokenTypes.reduce((result, item, index) => {
      const query = additionalTokenQueries[index];
      result[item.id] = {
        items: query?.data?.items.map(calculateUsdValue) || [],
        isOverflow: Boolean(query?.data?.next_page_params),
      };
      return result;
    }, {} as Record<string, { items: Array<TokenEnhancedData>; isOverflow: boolean }>);

    return {
      'ERC-20': {
        items: erc20query.data?.items.map(calculateUsdValue) || [],
        isOverflow: Boolean(erc20query.data?.next_page_params),
      },
      'ERC-721': {
        items: erc721query.data?.items.map(calculateUsdValue) || [],
        isOverflow: Boolean(erc721query.data?.next_page_params),
      },
      'ERC-1155': {
        items: erc1155query.data?.items.map(calculateUsdValue) || [],
        isOverflow: Boolean(erc1155query.data?.next_page_params),
      },
      'ERC-404': {
        items: erc404query.data?.items.map(calculateUsdValue) || [],
        isOverflow: Boolean(erc404query.data?.next_page_params),
      },
      ...additionalGroups,
    };
  }, [ additionalTokenQueries, erc1155query.data, erc20query.data, erc721query.data, erc404query.data ]);

  const isPending =
    erc20query.isPending ||
    erc721query.isPending ||
    erc1155query.isPending ||
    erc404query.isPending ||
    additionalTokenQueries.some((query) => query.isPending);

  const isError =
    erc20query.isError ||
    erc721query.isError ||
    erc1155query.isError ||
    erc404query.isError ||
    additionalTokenQueries.some((query) => query.isError);

  return {
    isPending,
    isError,
    data,
  };
}
