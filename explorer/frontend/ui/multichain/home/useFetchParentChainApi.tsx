import React from 'react';

import useFetch from 'lib/hooks/useFetch';

interface Params {
  path: string;
}

export default function useFetchParentChainApi() {
  const fetch = useFetch();
  return React.useCallback(({ path }: Params) => {
    // For current implementation of the feature we agreed to use the Ethereum mainnet stats data to show it on the homepage
    // It is very likely this will change once we identify the potential future for the Multichain explorer
    return fetch(`https://eth.blockscout.com/api/v2${ path }`);
  }, [ fetch ]);
}
