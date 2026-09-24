import { useCallback, useEffect } from 'react';

import type { Block } from 'types/api/block';
import type { EssentialDappsChainConfig } from 'types/client/marketplace';

import useApiFetch from 'lib/api/useApiFetch';

// Cache for block timestamp requests across the session
const timestampCache = new Map<string, Promise<number>>();
let activeInstances = 0;

export default function useGetBlockTimestamp() {
  const apiFetch = useApiFetch();

  // Clear entire cache when the last consumer unmounts
  useEffect(() => {
    activeInstances += 1;
    return () => {
      activeInstances -= 1;
      if (activeInstances === 0) {
        timestampCache.clear();
      }
    };
  }, []);

  return useCallback(async(
    chain: EssentialDappsChainConfig | undefined,
    blockNumber: bigint,
    signal?: AbortSignal,
  ): Promise<number> => {
    const cacheKey = `${ chain?.id }:${ blockNumber.toString() }`;
    const cached = timestampCache.get(cacheKey);
    if (cached) return cached;

    const response = (apiFetch('general:block', {
      pathParams: { height_or_hash: blockNumber.toString() },
      chain,
      fetchParams: {
        signal,
      },
    }) as Promise<Block>)
      .then((data) => data.timestamp ? Date.parse(data.timestamp) : 0)
      .catch((err) => {
        timestampCache.delete(cacheKey);
        throw err;
      });

    timestampCache.set(cacheKey, response);

    return response;
  }, [ apiFetch ]);
}
