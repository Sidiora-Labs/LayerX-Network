import { useQueryClient } from '@tanstack/react-query';
import React from 'react';

import type { OptimisticL2WithdrawalsItem, OptimisticL2WithdrawalsResponse } from 'types/api/optimisticL2';

import { getResourceKey } from 'lib/api/useApiQuery';
import { Skeleton } from 'toolkit/chakra/skeleton';

import OptimisticL2ClaimButton from './OptimisticL2ClaimButton';

interface Props {
  data: OptimisticL2WithdrawalsItem;
  isLoading?: boolean;
}

const OptimisticL2WithdrawalsItemStatus = ({ data, isLoading }: Props) => {

  const queryClient = useQueryClient();

  const handleClaimSuccess = React.useCallback((l1TxHash: string) => {
    queryClient.setQueriesData({
      queryKey: getResourceKey('general:optimistic_l2_withdrawals'),
      exact: false,
      type: 'active',
    },
    (prevData: OptimisticL2WithdrawalsResponse | undefined) => {
      if (!prevData) {
        return;
      }

      const newItems = prevData.items.map((withdrawal) => {
        if (`${ withdrawal.msg_nonce_version }-${ withdrawal.msg_nonce }` === `${ data.msg_nonce_version }-${ data.msg_nonce }`) {
          return {
            ...withdrawal,
            l1_transaction_hash: l1TxHash,
            status: 'Relayed',
          };
        }
        return withdrawal;
      });

      return {
        ...prevData,
        items: newItems,
      };
    });
  }, [ data.msg_nonce, data.msg_nonce_version, queryClient ]);

  if (data.status !== 'Ready for relay') {
    return <Skeleton loading={ isLoading } display="inline-block">{ data.status }</Skeleton>;
  }

  return <OptimisticL2ClaimButton data={ data } from={ data.from } onSuccess={ handleClaimSuccess } source="list"/>;
};

export default React.memo(OptimisticL2WithdrawalsItemStatus);
