import React from 'react';

import type { FormSubmitHandler } from './types';

import config from 'configs/app';

import useCallMethodPublicClient from './useCallMethodPublicClient';
import useCallMethodWalletClient from './useCallMethodWalletClient';

interface Params {
  addressHash: string;
}

function useFormSubmit({ addressHash }: Params): FormSubmitHandler {
  const callMethodWalletClient = useCallMethodWalletClient();
  const callMethodPublicClient = useCallMethodPublicClient();

  return React.useCallback(async(item, args, strategy) => {
    switch (strategy) {
      case 'read':
      case 'simulate': {
        return callMethodPublicClient({ args, item, addressHash, strategy });
      }
      case 'write': {
        return callMethodWalletClient({ args, item, addressHash });
      }

      default: {
        throw new Error(`Unknown call strategy "${ strategy }"`);
      }
    }
  }, [ addressHash, callMethodWalletClient, callMethodPublicClient ]);
}

function useFormSubmitFallback({ addressHash }: Params): FormSubmitHandler {
  const callMethodPublicClient = useCallMethodPublicClient();

  return React.useCallback(async(item, args, strategy) => {
    switch (strategy) {
      case 'read':
      case 'simulate': {
        return callMethodPublicClient({ args, item, addressHash, strategy });
      }

      default: {
        throw new Error(`Unknown call strategy "${ strategy }"`);
      }
    }

  }, [ callMethodPublicClient, addressHash ]);
}

const hook = config.features.blockchainInteraction.isEnabled ? useFormSubmit : useFormSubmitFallback;

export default hook;
