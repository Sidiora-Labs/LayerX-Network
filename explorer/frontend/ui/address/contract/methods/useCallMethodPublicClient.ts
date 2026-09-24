import React from 'react';
import { encodeFunctionData, getAddress } from 'viem';
import { usePublicClient } from 'wagmi';

import type { FormSubmitResult, MethodCallStrategy, SmartContractMethod } from './types';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import useAccount from 'lib/web3/useAccount';

import { getNativeCoinValue } from './utils';

interface Params {
  item: SmartContractMethod;
  args: Array<unknown>;
  addressHash: string;
  strategy: Exclude<MethodCallStrategy, 'write' | 'copy_calldata'>;
}

export default function useCallMethodPublicClient(): (params: Params) => Promise<FormSubmitResult> {
  const multichainContext = useMultichainContext();
  const chainId = Number((multichainContext?.chain.app_config ?? config).chain?.id);
  const publicClient = usePublicClient({ chainId });
  const { address: account } = useAccount();

  return React.useCallback(async({ args, item, addressHash, strategy }) => {
    if (!publicClient) {
      throw new Error('Public Client is not defined');
    }

    const address = getAddress(addressHash);

    // for payable methods we add additional input for native coin value
    const inputs = 'inputs' in item ? item.inputs : [];
    const _args = args.slice(0, inputs.length);
    const value = getNativeCoinValue(args[inputs.length]);

    if (item.type === 'fallback') {
      // if the fallback method acts as a read method, it can only have one input of type bytes
      // so we pass the input value as data without encoding it
      const data = typeof _args[0] === 'string' && _args[0].startsWith('0x') ? _args[0] as `0x${ string }` : undefined;
      const result = await publicClient.call({
        account,
        to: address,
        value,
        data,
      });
      const estimatedGas = await publicClient.estimateGas({
        account,
        to: address,
        value,
        data,
      });

      return {
        source: 'public_client' as const,
        data: result.data,
        estimatedGas,
      };
    }

    if (item.type === 'receive') {
      const estimatedGas = await publicClient.estimateGas({
        account,
        to: address,
        value,
      });

      return {
        source: 'public_client' as const,
        data: undefined,
        estimatedGas,
      };
    }

    const params = {
      abi: [ item ],
      functionName: item.name,
      args: _args,
      address,
      account,
      value,
    };
    const paramsForGasEstimate = {
      account,
      value,
      to: address,
      data: encodeFunctionData({
        abi: [ item ],
        functionName: item.name,
        args: _args,
      }),
    };

    const result = strategy === 'read' ? await publicClient.readContract(params) : await publicClient.simulateContract(params);
    const estimatedGas = strategy === 'simulate' ? await publicClient.estimateGas(paramsForGasEstimate) : undefined;

    return {
      source: 'public_client' as const,
      data: strategy === 'read' ? result : result.result,
      estimatedGas,
    };

  }, [ account, publicClient ]);
}
