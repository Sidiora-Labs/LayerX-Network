import { useQueryClient } from '@tanstack/react-query';
import React from 'react';
import { useSendTransaction, useSwitchChain } from 'wagmi';

import type { ArbitrumL2MessageClaimResponse, ArbitrumL2TxnWithdrawalsResponse } from 'types/api/arbitrumL2';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import useApiFetch from 'lib/api/useApiFetch';
import { getResourceKey } from 'lib/api/useApiQuery';
import capitalizeFirstLetter from 'lib/capitalizeFirstLetter';
import getErrorMessage from 'lib/errors/getErrorMessage';
import getErrorObjPayload from 'lib/errors/getErrorObjPayload';
import getErrorProp from 'lib/errors/getErrorProp';
import useWallet from 'lib/web3/useWallet';
import { Button } from 'toolkit/chakra/button';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { toaster } from 'toolkit/chakra/toaster';

import ArbitrumL2TxnWithdrawalsClaimTx from './ArbitrumL2TxnWithdrawalsClaimTx';

const rollupFeature = config.features.rollup;

interface Props {
  messageId: number;
  txHash: string | undefined;
  completionTxHash?: string;
  isLoading?: boolean;
}

const ArbitrumL2TxnWithdrawalsClaimButton = ({ messageId, txHash, completionTxHash, isLoading: isDataLoading }: Props) => {
  const [ isPending, setIsPending ] = React.useState(false);
  const [ claimTxHash, setClaimTxHash ] = React.useState<string | undefined>(completionTxHash);
  const apiFetch = useApiFetch();

  const { sendTransactionAsync } = useSendTransaction();
  const { switchChainAsync } = useSwitchChain();
  const queryClient = useQueryClient();

  const sendClaimTx = React.useCallback(async() => {

    if (!rollupFeature.isEnabled) {
      return;
    }

    try {
      setIsPending(true);

      const response = await apiFetch<'general:arbitrum_l2_message_claim', ArbitrumL2MessageClaimResponse, ResourceError<unknown>>(
        'general:arbitrum_l2_message_claim',
        { pathParams: { id: messageId.toString() },
        });

      if ('calldata' in response) {
        await switchChainAsync({ chainId: Number(rollupFeature.parentChain.id) });

        const hash = await sendTransactionAsync({
          data: response.calldata as `0x${ string }`,
          to: response.outbox_address_hash as `0x${ string }`,
        });

        setClaimTxHash(hash);
      }
    } catch (error) {
      const apiError = getErrorObjPayload<{ message: string }>(error);
      const message = capitalizeFirstLetter(apiError?.message || getErrorProp(error, 'shortMessage') || getErrorMessage(error) || 'Something went wrong');
      toaster.error({
        title: 'Error',
        description: message,
      });
      setIsPending(false);
    }
  }, [ apiFetch, messageId, sendTransactionAsync, switchChainAsync ]);

  const web3Wallet = useWallet({ source: 'Smart contracts', onConnect: sendClaimTx });

  const handleClaimClick = React.useCallback(async() => {
    if (!web3Wallet.address) {
      web3Wallet.connect();
    } else {
      sendClaimTx();
    }
  }, [ sendClaimTx, web3Wallet ]);

  const handleSuccess = React.useCallback(() => {
    queryClient.setQueryData(
      getResourceKey('general:arbitrum_l2_txn_withdrawals', { pathParams: { hash: txHash } }),
      (prevData: ArbitrumL2TxnWithdrawalsResponse | undefined) => {
        if (!prevData) {
          return;
        }

        const newItems = prevData.items.map(item => item.id === messageId ? { ...item, status: 'relayed' } : item);

        return {
          ...prevData,
          items: newItems,
        };
      });
    setIsPending(false);
  }, [ messageId, queryClient, txHash ]);

  const handleError = React.useCallback((error: Error) => {
    toaster.error({
      title: 'Error',
      description: error.message,
    });
    setIsPending(false);
  }, [ ]);

  if (claimTxHash) {
    return (
      <ArbitrumL2TxnWithdrawalsClaimTx
        isPending={ isPending }
        hash={ claimTxHash }
        onSuccess={ handleSuccess }
        onError={ handleError }
      />
    );
  }

  const isLoading = isPending || web3Wallet.isOpen;

  return (
    <Skeleton loading={ isDataLoading }>
      <Button
        size="sm"
        variant="outline"
        onClick={ handleClaimClick }
        loading={ isLoading }
        loadingText="Claim"
      >
        Claim
      </Button>
    </Skeleton>
  );
};

export default React.memo(ArbitrumL2TxnWithdrawalsClaimButton);
