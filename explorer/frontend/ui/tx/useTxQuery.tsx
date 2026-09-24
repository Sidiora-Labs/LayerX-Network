import type { UseQueryResult } from '@tanstack/react-query';
import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import type { SocketMessage } from 'lib/socket/types';
import type { Transaction } from 'types/api/transaction';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import useApiQuery, { getResourceKey } from 'lib/api/useApiQuery';
import { retry } from 'lib/api/useQueryClientConfig';
import delay from 'lib/delay';
import getQueryParamString from 'lib/router/getQueryParamString';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { TX, TX_ZKEVM_L2 } from 'stubs/tx';
import { SECOND } from 'toolkit/utils/consts';

const rollupFeature = config.features.rollup;

export type TxQuery = UseQueryResult<Transaction, ResourceError<{ status: number }>> & {
  socketStatus: 'close' | 'error' | undefined;
  setRefetchEnabled: (value: boolean) => void;
};

interface Params {
  hash?: string;
  isEnabled?: boolean;
}

export default function useTxQuery(params?: Params): TxQuery {
  const [ socketStatus, setSocketStatus ] = React.useState<'close' | 'error'>();
  const [ isRefetchEnabled, setRefetchEnabled ] = React.useState(false);

  const router = useRouter();
  const queryClient = useQueryClient();

  const hash = params?.hash ?? getQueryParamString(router.query.hash);

  const queryResult = useApiQuery<'general:tx', { status: number }>('general:tx', {
    pathParams: { hash },
    queryOptions: {
      enabled: Boolean(hash) && params?.isEnabled !== false,
      refetchOnMount: false,
      placeholderData: rollupFeature.isEnabled && rollupFeature.type === 'zkEvm' ? TX_ZKEVM_L2 : TX,
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
  const { data, isError, isPlaceholderData, isPending } = queryResult;

  const handleStatusUpdateMessage: SocketMessage.TxStatusUpdate['handler'] = React.useCallback(async() => {
    await delay(5 * SECOND);
    queryClient.invalidateQueries({
      queryKey: getResourceKey('general:tx', { pathParams: { hash } }),
    });
  }, [ queryClient, hash ]);

  const handleSocketClose = React.useCallback(() => {
    setSocketStatus('close');
  }, []);

  const handleSocketError = React.useCallback(() => {
    setSocketStatus('error');
  }, []);

  const channel = useSocketChannel({
    topic: `transactions:${ hash }`,
    onSocketClose: handleSocketClose,
    onSocketError: handleSocketError,
    isDisabled: isPending || isPlaceholderData || isError || data.status !== null,
  });
  useSocketMessage({
    channel,
    event: 'collated',
    handler: handleStatusUpdateMessage,
  });

  return React.useMemo(() => ({
    ...queryResult,
    socketStatus,
    setRefetchEnabled,
  }), [ queryResult, socketStatus, setRefetchEnabled ]);
}
