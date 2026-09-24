import { useRouter } from 'next/router';
import React from 'react';

import type { SocketMessage } from 'lib/socket/types';
import type { RawTracesResponse } from 'types/api/rawTrace';

import useApiQuery from 'lib/api/useApiQuery';
import getQueryParamString from 'lib/router/getQueryParamString';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { TX_RAW_TRACE } from 'stubs/tx';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import RawDataSnippet from 'ui/shared/RawDataSnippet';
import TxPendingAlert from 'ui/tx/TxPendingAlert';
import TxSocketAlert from 'ui/tx/TxSocketAlert';

import type { TxQuery } from './useTxQuery';

interface Props {
  txQuery: TxQuery;
}

const TxRawTrace = ({ txQuery }: Props) => {
  const [ isQueryEnabled, setIsQueryEnabled ] = React.useState(false);
  const [ rawTraces, setRawTraces ] = React.useState<RawTracesResponse>();
  const router = useRouter();
  const hash = getQueryParamString(router.query.hash);

  const { data, isPlaceholderData, isError } = useApiQuery('general:tx_raw_trace', {
    pathParams: { hash },
    queryOptions: {
      enabled: Boolean(hash) && Boolean(txQuery.data?.status) && isQueryEnabled,
      placeholderData: TX_RAW_TRACE,
    },
  });

  const handleRawTraceMessage: SocketMessage.TxRawTrace['handler'] = React.useCallback((payload) => {
    setRawTraces(payload);
  }, [ ]);

  const enableQuery = React.useCallback(() => setIsQueryEnabled(true), []);

  const channel = useSocketChannel({
    topic: `transactions:${ hash }`,
    isDisabled: !hash || txQuery.isPlaceholderData || !txQuery.data?.status,
    onJoin: enableQuery,
    onSocketError: enableQuery,
  });
  useSocketMessage({
    channel,
    event: 'raw_trace',
    handler: handleRawTraceMessage,
  });

  if (!txQuery.isPending && !txQuery.isPlaceholderData && !txQuery.isError && !txQuery.data.status) {
    return txQuery.socketStatus ? <TxSocketAlert status={ txQuery.socketStatus }/> : <TxPendingAlert/>;
  }

  if (isError || txQuery.isError) {
    return <DataFetchAlert/>;
  }

  const dataToDisplay = rawTraces ? rawTraces : data;

  if (!isPlaceholderData && dataToDisplay?.length === 0) {
    return <span>No trace entries found.</span>;
  }

  const text = JSON.stringify(dataToDisplay, undefined, 4);

  return <RawDataSnippet data={ text } isLoading={ isPlaceholderData } textareaMaxHeight={{ base: '400px', lg: '600px' }}/>;
};

export default TxRawTrace;
