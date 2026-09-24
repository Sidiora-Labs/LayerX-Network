import type { UseQueryResult } from '@tanstack/react-query';
import { useQuery } from '@tanstack/react-query';

import type { AddressCounters } from 'types/api/address';
import type { ClusterChainConfig } from 'types/multichain';

import type { ResourceError } from 'lib/api/resources';
import useApiQuery from 'lib/api/useApiQuery';
import { publicClient } from 'lib/web3/client';
import { ADDRESS_COUNTERS } from 'stubs/address';
import { GET_TRANSACTIONS_COUNT } from 'stubs/RPC';

type RpcResponseType = [
  number | null,
];

export type AddressCountersQuery = UseQueryResult<AddressCounters, ResourceError<{ status: number }>> & {
  isDegradedData: boolean;
};

interface Params {
  hash: string;
  isEnabled?: boolean;
  isLoading?: boolean;
  isDegradedData?: boolean;
  chain?: ClusterChainConfig;
}

export default function useAddressCountersQuery({ hash, isLoading, isDegradedData, isEnabled = true, chain }: Params): AddressCountersQuery {
  const enabled = isEnabled && Boolean(hash) && !isLoading;

  const apiQuery = useApiQuery<'general:address_counters', { status: number }>('general:address_counters', {
    pathParams: { hash },
    queryOptions: {
      enabled: enabled && !isDegradedData,
      placeholderData: ADDRESS_COUNTERS,
      refetchOnMount: false,
    },
    chain,
  });

  const rpcQuery = useQuery<RpcResponseType, unknown, AddressCounters | null>({
    queryKey: [ 'RPC', 'address_counters', { hash } ],
    queryFn: async() => {
      if (!publicClient) {
        throw new Error('No public RPC client');
      }

      const txCount = publicClient.getTransactionCount({ address: hash as `0x${ string }` }).catch(() => null);

      return Promise.all([
        txCount,
      ]);
    },
    select: (response) => {
      const [ txCount ] = response;
      return {
        transactions_count: txCount?.toString() ?? '0',
        token_transfers_count: '0',
        gas_usage_count: null,
        validations_count: null,
      };
    },
    placeholderData: [ GET_TRANSACTIONS_COUNT ],
    enabled: enabled && (isDegradedData || apiQuery.isError),
    retry: false,
    refetchOnMount: false,
  });

  const isRpcQuery = Boolean((isDegradedData || apiQuery.isError) && rpcQuery.data && publicClient);
  const query = isRpcQuery ? rpcQuery as UseQueryResult<AddressCounters, ResourceError<{ status: number }>> : apiQuery;

  return {
    ...query,
    isDegradedData: isRpcQuery,
  };
}
