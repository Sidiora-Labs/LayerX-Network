import { useRouter } from 'next/router';
import React from 'react';

import type { TxsSocketType } from './types';

import useGradualIncrement from 'lib/hooks/useGradualIncrement';
import getQueryParamString from 'lib/router/getQueryParamString';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';

function getSocketParams(type: TxsSocketType, page: string) {

  switch (type) {
    case 'txs_home': {
      return { topic: 'transactions:new_transaction' as const, event: 'transaction' as const };
    }
    case 'txs_validated': {
      return !page || page === '1' ? { topic: 'transactions:new_transaction' as const, event: 'transaction' as const } : {};
    }
    case 'txs_pending': {
      return !page || page === '1' ? { topic: 'transactions:new_pending_transaction' as const, event: 'pending_transaction' as const } : {};
    }
    default:
      return {};
  }
}

function assertIsNewTxResponse(response: unknown): response is { transaction: number } {
  return typeof response === 'object' && response !== null && 'transaction' in response;
}
function assertIsNewPendingTxResponse(response: unknown): response is { pending_transaction: number } {
  return typeof response === 'object' && response !== null && 'pending_transaction' in response;
}

interface Params {
  type: TxsSocketType;
  isLoading?: boolean;
}

export default function useNewTxsSocketTypeAll({ type, isLoading }: Params) {
  const router = useRouter();
  const page = getQueryParamString(router.query.page);

  const [ num, setNum ] = useGradualIncrement(0);
  const [ showErrorAlert, setShowErrorAlert ] = React.useState(false);

  const { topic, event } = getSocketParams(type, page);

  const handleNewTxMessage = React.useCallback((response: { transaction: number } | { pending_transaction: number } | unknown) => {
    if (assertIsNewTxResponse(response)) {
      setNum(response.transaction);
    }
    if (assertIsNewPendingTxResponse(response)) {
      setNum(response.pending_transaction);
    }
  }, [ setNum ]);

  const handleSocketClose = React.useCallback(() => {
    setShowErrorAlert(true);
  }, []);

  const handleSocketError = React.useCallback(() => {
    setShowErrorAlert(true);
  }, []);

  const channel = useSocketChannel({
    topic,
    onSocketClose: handleSocketClose,
    onSocketError: handleSocketError,
    isDisabled: !topic || Boolean(isLoading),
  });

  useSocketMessage({
    channel,
    event,
    handler: handleNewTxMessage,
  });

  if (!topic && !event) {
    return { };
  }

  return { num, showErrorAlert };
}
