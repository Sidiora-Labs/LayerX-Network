import type { TypedData } from 'abitype';
import { useCallback } from 'react';
import type { Account, SignTypedDataParameters } from 'viem';
import { useAccount, useSendTransaction, useSwitchChain, useSignMessage, useSignTypedData } from 'wagmi';

import config from 'configs/app';
import useRewardsActivity from 'lib/hooks/useRewardsActivity';
import * as mixpanel from 'lib/mixpanel/index';

type SendTransactionArgs = {
  chainId?: number;
  mode?: 'prepared';
  to: `0x${ string }` | null;
};

export type SignTypedDataArgs<
  TTypedData extends
  | TypedData |
  {
    [key: string]: unknown;
  } = TypedData,
  TPrimaryType extends string = string,
> = SignTypedDataParameters<TTypedData, TPrimaryType, Account>;

export default function useMarketplaceWallet(appId: string, isEssentialDapp = false) {
  const { address, chainId } = useAccount();
  const { sendTransactionAsync } = useSendTransaction();
  const { signMessageAsync } = useSignMessage();
  const { signTypedDataAsync } = useSignTypedData();
  const { switchChainAsync } = useSwitchChain();
  const { trackTransaction, trackTransactionConfirm } = useRewardsActivity();

  const logEvent = useCallback((event: mixpanel.EventPayload<mixpanel.EventTypes.WALLET_ACTION>['Action']) => {
    mixpanel.logEvent(mixpanel.EventTypes.WALLET_ACTION, {
      Action: event,
      Address: address,
      AppId: appId,
      Source: isEssentialDapp ? 'Essential dapps' : 'Dappscout',
      ChainId: isEssentialDapp ? String(chainId) : undefined,
    });
  }, [ address, appId, chainId, isEssentialDapp ]);

  const switchChain = useCallback(
    (chainId: number) => switchChainAsync({ chainId }),
    [ switchChainAsync ],
  );

  const checkAndSwitchChain = useCallback(async() => {
    if (!isEssentialDapp && Number(config.chain.id) !== chainId) {
      await switchChain(Number(config.chain.id));
    }
  }, [ chainId, switchChain, isEssentialDapp ]);

  const sendTransaction = useCallback(async(transaction: SendTransactionArgs) => {
    await checkAndSwitchChain();
    const activityResponse = await trackTransaction(
      address ?? '',
      transaction.to ?? '',
      isEssentialDapp ? String(chainId) : undefined,
    );
    const tx = await sendTransactionAsync(transaction);
    if (activityResponse?.token) {
      await trackTransactionConfirm(tx, activityResponse.token);
    }
    logEvent('Send Transaction');
    return tx;
  }, [
    sendTransactionAsync, logEvent, trackTransaction, trackTransactionConfirm,
    address, checkAndSwitchChain, chainId, isEssentialDapp,
  ]);

  const signMessage = useCallback(async(message: string) => {
    await checkAndSwitchChain();
    const signature = await signMessageAsync({ message });
    logEvent('Sign Message');
    return signature;
  }, [ signMessageAsync, logEvent, checkAndSwitchChain ]);

  const signTypedData = useCallback(async(typedData: SignTypedDataArgs) => {
    await checkAndSwitchChain();
    if (typedData.domain) {
      typedData.domain.chainId = Number(typedData.domain.chainId);
    }
    const signature = await signTypedDataAsync(typedData);
    logEvent('Sign Typed Data');
    return signature;
  }, [ signTypedDataAsync, logEvent, checkAndSwitchChain ]);

  return {
    address,
    chainId,
    sendTransaction,
    signMessage,
    signTypedData,
    switchChain,
  };
}
