import type { UseQueryResult } from '@tanstack/react-query';
import { useQuery } from '@tanstack/react-query';
import React from 'react';
import type { Chain, GetBlockReturnType } from 'viem';

import type { BlockWithdrawalsResponse } from 'types/api/block';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import { retry } from 'lib/api/useQueryClientConfig';
import hexToDecimal from 'lib/hexToDecimal';
import { publicClient } from 'lib/web3/client';
import { GET_BLOCK } from 'stubs/RPC';
import { generateListStub } from 'stubs/utils';
import { WITHDRAWAL } from 'stubs/withdrawals';
import { SECOND } from 'toolkit/utils/consts';
import { unknownAddress } from 'ui/shared/address/utils';
import type { QueryWithPagesResult } from 'ui/shared/pagination/useQueryWithPages';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import { emptyPagination } from 'ui/shared/pagination/utils';

import type { BlockQuery } from './useBlockQuery';

type RpcResponseType = GetBlockReturnType<Chain, false, 'latest'> | null;

export type BlockWithdrawalsQuery = QueryWithPagesResult<'general:block_withdrawals'> & {
  isDegradedData: boolean;
};

interface Params {
  heightOrHash: string;
  blockQuery: BlockQuery;
  tab: string;
}

export default function useBlockWithdrawalsQuery({ heightOrHash, blockQuery, tab }: Params): BlockWithdrawalsQuery {
  const [ isRefetchEnabled, setRefetchEnabled ] = React.useState(false);

  const apiQuery = useQueryWithPages({
    resourceName: 'general:block_withdrawals',
    pathParams: { height_or_hash: heightOrHash },
    options: {
      enabled:
        tab === 'withdrawals' &&
        config.features.beaconChain.isEnabled &&
        !blockQuery.isPlaceholderData && !blockQuery.isDegradedData,
      placeholderData: generateListStub<'general:block_withdrawals'>(WITHDRAWAL, 50, { next_page_params: {
        index: 5,
        items_count: 50,
      } }),
      refetchOnMount: false,
      retry: (failureCount, error) => {
        if (isRefetchEnabled) {
          return false;
        }

        return retry(failureCount, error);
      },
      refetchInterval: (): number | false => {
        return isRefetchEnabled ? 15 * SECOND : false;
      },
    },
  });

  const rpcQuery = useQuery<RpcResponseType, unknown, BlockWithdrawalsResponse | null>({
    queryKey: [ 'RPC', 'block', { heightOrHash } ],
    queryFn: async() => {
      if (!publicClient) {
        return null;
      }

      const blockParams = heightOrHash.startsWith('0x') ? { blockHash: heightOrHash as `0x${ string }` } : { blockNumber: BigInt(heightOrHash) };
      return publicClient.getBlock(blockParams).catch(() => null);
    },
    select: (block) => {
      if (!block) {
        return null;
      }

      return {
        items: block.withdrawals
          ?.map((withdrawal) => {
            return {
              amount: hexToDecimal(withdrawal.amount).toString(),
              index: hexToDecimal(withdrawal.index),
              validator_index: hexToDecimal(withdrawal.validatorIndex),
              receiver: { ...unknownAddress, hash: withdrawal.address },
            };
          })
          .sort((a, b) => b.index - a.index) ?? [],
        next_page_params: null,
      };
    },
    placeholderData: GET_BLOCK,
    enabled:
      publicClient !== undefined &&
      tab === 'withdrawals' &&
      config.features.beaconChain.isEnabled &&
      (blockQuery.isDegradedData || apiQuery.isError || apiQuery.errorUpdateCount > 0),
    retry: false,
    refetchOnMount: false,
  });

  React.useEffect(() => {
    if (apiQuery.isPlaceholderData || !publicClient) {
      return;
    }

    if (apiQuery.isError && apiQuery.errorUpdateCount === 1) {
      setRefetchEnabled(true);
    } else if (!apiQuery.isError) {
      setRefetchEnabled(false);
    }
  }, [ apiQuery.errorUpdateCount, apiQuery.isError, apiQuery.isPlaceholderData ]);

  React.useEffect(() => {
    if (!rpcQuery.isPlaceholderData && !rpcQuery.data) {
      setRefetchEnabled(false);
    }
  }, [ rpcQuery.data, rpcQuery.isPlaceholderData ]);

  const isRpcQuery = Boolean((
    blockQuery.isDegradedData ||
    ((apiQuery.isError || apiQuery.isPlaceholderData) && apiQuery.errorUpdateCount > 0)
  ) && rpcQuery.data && publicClient);

  const rpcQueryWithPages: QueryWithPagesResult<'general:block_withdrawals'> = {
    ...rpcQuery as UseQueryResult<BlockWithdrawalsResponse, ResourceError>,
    pagination: emptyPagination,
    onFilterChange: () => {},
    onSortingChange: () => {},
    chainValue: undefined,
    onChainValueChange: () => {},
  };

  const query = isRpcQuery ? rpcQueryWithPages : apiQuery;

  return {
    ...query,
    isDegradedData: isRpcQuery,
  };
}
