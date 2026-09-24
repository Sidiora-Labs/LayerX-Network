import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import type { SocketMessage } from 'lib/socket/types';
import type { AddressCoinBalanceHistoryResponse } from 'types/api/address';

import { getResourceKey } from 'lib/api/useApiQuery';
import useIsMounted from 'lib/hooks/useIsMounted';
import getQueryParamString from 'lib/router/getQueryParamString';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { ADDRESS_COIN_BALANCE } from 'stubs/address';
import { generateListStub } from 'stubs/utils';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import SocketAlert from 'ui/shared/SocketAlert';

import AddressCoinBalanceChart from './coinBalance/AddressCoinBalanceChart';
import AddressCoinBalanceHistory from './coinBalance/AddressCoinBalanceHistory';

type Props = {
  shouldRender?: boolean;
  isQueryEnabled?: boolean;
};

const AddressCoinBalance = ({ shouldRender = true, isQueryEnabled = true }: Props) => {
  const [ socketAlert, setSocketAlert ] = React.useState(false);
  const queryClient = useQueryClient();
  const router = useRouter();
  const isMounted = useIsMounted();

  const scrollRef = React.useRef<HTMLDivElement>(null);

  const addressHash = getQueryParamString(router.query.hash);
  const coinBalanceQuery = useQueryWithPages({
    resourceName: 'general:address_coin_balance',
    pathParams: { hash: addressHash },
    scrollRef,
    options: {
      enabled: isQueryEnabled,
      placeholderData: generateListStub<'general:address_coin_balance'>(
        ADDRESS_COIN_BALANCE,
        50,
        {
          next_page_params: {
            block_number: 8009880,
            items_count: 50,
          },
        },
      ),
    },
  });

  const handleSocketError = React.useCallback(() => {
    setSocketAlert(true);
  }, []);

  const handleNewSocketMessage: SocketMessage.AddressCoinBalance['handler'] = React.useCallback((payload) => {
    setSocketAlert(false);

    queryClient.setQueryData(
      getResourceKey('general:address_coin_balance', { pathParams: { hash: addressHash } }),
      (prevData: AddressCoinBalanceHistoryResponse | undefined) => {
        if (!prevData) {
          return;
        }

        return {
          ...prevData,
          items: [
            payload.coin_balance,
            ...prevData.items,
          ],
        };
      });
  }, [ addressHash, queryClient ]);

  const channel = useSocketChannel({
    topic: `addresses:${ addressHash.toLowerCase() }`,
    onSocketClose: handleSocketError,
    onSocketError: handleSocketError,
    isDisabled: !addressHash || coinBalanceQuery.isPlaceholderData || coinBalanceQuery.pagination.page !== 1,
  });
  useSocketMessage({
    channel,
    event: 'coin_balance',
    handler: handleNewSocketMessage,
  });

  if (!isMounted || !shouldRender) {
    return null;
  }

  return (
    <>
      { socketAlert && <SocketAlert mb={ 6 }/> }
      <AddressCoinBalanceChart addressHash={ addressHash }/>
      <div ref={ scrollRef }></div>
      <AddressCoinBalanceHistory query={ coinBalanceQuery }/>
    </>
  );
};

export default React.memo(AddressCoinBalance);
