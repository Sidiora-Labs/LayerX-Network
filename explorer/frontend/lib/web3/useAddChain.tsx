import React from 'react';
import type { AddEthereumChainParameter } from 'viem';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import { SECOND } from 'toolkit/utils/consts';

import useRewardsActivity from '../hooks/useRewardsActivity';
import useProvider from './useProvider';
import { getHexadecimalChainId } from './utils';

function getParams(chainConfig: typeof config): AddEthereumChainParameter {
  if (!chainConfig.chain.id) {
    throw new Error('Missing required chain config');
  }

  return {
    chainId: getHexadecimalChainId(Number(chainConfig.chain.id)),
    chainName: chainConfig.chain.name ?? '',
    nativeCurrency: {
      name: chainConfig.chain.currency.name ?? '',
      symbol: chainConfig.chain.currency.symbol ?? '',
      decimals: chainConfig.chain.currency.decimals ?? 18,
    },
    rpcUrls: chainConfig.chain.rpcUrls,
    blockExplorerUrls: [ chainConfig.app.baseUrl ],
  };
}

interface Params {
  chainConfig?: typeof config;
}

export default function useAddChain(params?: Params) {
  const { data: { wallet, provider } = {} } = useProvider();
  const { trackUsage } = useRewardsActivity();
  const multichainContext = useMultichainContext();

  const chainConfig = params?.chainConfig || multichainContext?.chain.app_config || config;

  return React.useCallback(async() => {
    if (!wallet || !provider) {
      throw new Error('Wallet or provider not found');
    }

    const start = Date.now();

    await provider.request({
      method: 'wallet_addEthereumChain',
      params: [ getParams(chainConfig) ],
    });

    // if network is already added, the promise resolves immediately
    if (Date.now() - start > SECOND) {
      await trackUsage('add_network');
    }
  }, [ wallet, provider, chainConfig, trackUsage ]);
}
