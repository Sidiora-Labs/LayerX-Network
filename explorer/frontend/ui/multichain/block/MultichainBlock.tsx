import { useRouter } from 'next/router';
import React from 'react';

import multichainConfig from 'configs/multichain';
import { MultichainProvider } from 'lib/contexts/multichain';
import getQueryParamString from 'lib/router/getQueryParamString';
import Block from 'ui/pages/Block';

const MultichainBlock = () => {
  const router = useRouter();
  const chainSlug = getQueryParamString(router.query.chain_slug);
  const chainData = multichainConfig()?.chains.find(chain => chain.slug === chainSlug);

  return (
    <MultichainProvider chainId={ chainData?.id }>
      <Block/>
    </MultichainProvider>
  );
};

export default React.memo(MultichainBlock);
