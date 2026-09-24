import { useRouter } from 'next/router';
import React from 'react';

import getQueryParamString from 'lib/router/getQueryParamString';

interface Props {
  chainIds: Array<string>;
}
export default function useChainSelectErc20({ chainIds }: Props) {
  const router = useRouter();
  const chainId = getQueryParamString(router.query.chain_id);

  return React.useState<Array<string> | undefined>(
    chainId && chainIds.includes(chainId) ? [ chainId ] : [ 'all' ],
  );
}
