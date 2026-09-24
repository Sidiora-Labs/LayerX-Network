import { get } from 'es-toolkit/compat';
import React from 'react';

import type config from 'configs/app';
import getErrorObj from 'lib/errors/getErrorObj';

import useAddChain from './useAddChain';
import useProvider from './useProvider';
import useSwitchChain from './useSwitchChain';

interface Params {
  chainConfig?: typeof config;
}

export default function useSwitchOrAddChain(params?: Params) {
  const { data: { wallet, provider } = {} } = useProvider();
  const addChain = useAddChain(params);
  const switchChain = useSwitchChain(params);

  return React.useCallback(async() => {
    if (!wallet || !provider) {
      return;
    }

    try {
      return switchChain();
    } catch (error) {
      const errorObj = getErrorObj(error);
      const code = get(errorObj, 'code');
      const originalErrorCode = get(errorObj, 'data.originalError.code');

      // This error code indicates that the chain has not been added to Wallet.
      if (code === 4902 || originalErrorCode === 4902) {
        return addChain();
      }

      throw error;
    }
  }, [ addChain, provider, wallet, switchChain ]);
}
