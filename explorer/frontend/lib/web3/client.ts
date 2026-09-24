import { createPublicClient, http } from 'viem';

import { currentChain } from './chains';

export const publicClient = (() => {
  if (currentChain?.rpcUrls.default.http.filter(Boolean).length === 0) {
    return;
  }

  try {
    return createPublicClient({
      chain: currentChain,
      transport: http(),
      batch: {
        multicall: true,
      },
    });
  } catch (error) {}
})();
