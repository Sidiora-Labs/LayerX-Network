import React from 'react';
import { getAddress, type Abi } from 'viem';
import { useAccount, useSwitchChain, useWriteContract, useSendTransaction } from 'wagmi';

import type { FormSubmitResult, SmartContractMethod } from './types';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import useRewardsActivity from 'lib/hooks/useRewardsActivity';
import useWallet from 'lib/web3/useWallet';

import { getNativeCoinValue } from './utils';

const feature = config.features.blockchainInteraction;

interface Params {
  item: SmartContractMethod;
  args: Array<unknown>;
  addressHash: string;
}

export default function useCallMethodWalletClient(): (params: Params) => Promise<FormSubmitResult> {
  const multichainContext = useMultichainContext();
  const chainConfig = (multichainContext?.chain.app_config ?? config).chain;
  const targetChainId = chainConfig?.id ? Number(chainConfig.id) : undefined;

  const { isConnected, chainId, address: account } = useAccount();
  const { switchChainAsync } = useSwitchChain();
  const { trackTransaction, trackTransactionConfirm } = useRewardsActivity();
  const { writeContractAsync } = useWriteContract();
  const { sendTransactionAsync } = useSendTransaction();

  const { type: walletType } = useWallet({ source: 'Smart contracts' });

  return React.useCallback(async({ args, item, addressHash }) => {
    if (!isConnected) {
      throw new Error('Wallet is not connected');
    }

    if (chainId && targetChainId && chainId !== targetChainId) {
      await switchChainAsync({ chainId: targetChainId });
    }

    const address = getAddress(addressHash);
    const activityResponse = await trackTransaction(account ?? '', address);

    // for payable methods we add additional input for native coin value
    const inputs = 'inputs' in item ? item.inputs : [];
    const _args = args.slice(0, inputs.length);
    const value = getNativeCoinValue(args[inputs.length]);

    if (item.type === 'receive' || item.type === 'fallback') {
      // if the fallback method acts as a read method, it can only have one input of type bytes
      // so we pass the input value as data without encoding it
      const data = typeof _args[0] === 'string' && _args[0].startsWith('0x') ? _args[0] as `0x${ string }` : undefined;

      // seems like Dynamic WaaS (assigned when user signed up with email) does not estimate gas for transactions
      // so we pass 0 as gas here
      const estimatedGas = feature.isEnabled && feature.connectorType === 'dynamic' && walletType === 'dynamicwaas' ?
        BigInt(0) : undefined;

      const hash = await sendTransactionAsync({
        to: address,
        value,
        gas: estimatedGas,
        chainId: targetChainId,
        ...(data ? { data } : {}),
      });

      if (activityResponse?.token) {
        await trackTransactionConfirm(hash, activityResponse.token);
      }

      return { source: 'wallet_client', data: { hash } };
    }

    const methodName = item.name;

    if (!methodName) {
      throw new Error('Method name is not defined');
    }

    // seems like Dynamic WaaS (assigned when user signed up with email) does not estimate gas for transactions
    // so we pass 0 as gas here
    const estimatedGas = feature.isEnabled && feature.connectorType === 'dynamic' && walletType === 'dynamicwaas' ?
      BigInt(0) : undefined;

    const hash = await writeContractAsync({
      args: _args,
      // Here we provide the ABI as an array containing only one item from the submitted form.
      // This is a workaround for the issue with the "viem" library.
      // It lacks a "method_id" field to uniquely identify the correct method and instead attempts to find a method based on its name.
      // But the name is not unique in the contract ABI and this behavior in the "viem" could result in calling the wrong method.
      // See related issues:
      //    - https://github.com/blockscout/frontend/issues/1032,
      //    - https://github.com/blockscout/frontend/issues/1327
      abi: [ item ] as Abi,
      functionName: methodName,
      address,
      value,
      account,
      gas: estimatedGas,
      chainId: targetChainId,
    });

    if (activityResponse?.token) {
      await trackTransactionConfirm(hash, activityResponse.token);
    }

    return { source: 'wallet_client', data: { hash } };
  }, [
    isConnected,
    chainId,
    targetChainId,
    trackTransaction,
    account,
    walletType,
    writeContractAsync,
    switchChainAsync,
    sendTransactionAsync,
    trackTransactionConfirm,
  ]);
}
