import { useRouter } from 'next/router';
import React from 'react';

import multichainConfig from 'configs/multichain';
import { MultichainProvider } from 'lib/contexts/multichain';
import getQueryParamString from 'lib/router/getQueryParamString';
import Transaction from 'ui/pages/Transaction';

const MultichainTx = () => {
  const router = useRouter();
  const chainSlug = getQueryParamString(router.query.chain_slug);
  const chainData = multichainConfig()?.chains.find(chain => chain.slug === chainSlug);

  return (
    <MultichainProvider chainId={ chainData?.id }>
      <Transaction/>
    </MultichainProvider>
  );
};

export default React.memo(MultichainTx);
