import { useRouter } from 'next/router';
import React from 'react';

import multichainConfig from 'configs/multichain';
import getSocketUrl from 'lib/api/getSocketUrl';
import { MultichainProvider } from 'lib/contexts/multichain';
import getQueryParamString from 'lib/router/getQueryParamString';
import { SocketProvider } from 'lib/socket/context';
import Token from 'ui/pages/Token';

const MultichainToken = () => {
  const router = useRouter();
  const chainSlug = getQueryParamString(router.query.chain_slug);
  const chainData = multichainConfig()?.chains.find(chain => chain.slug === chainSlug);

  return (
    <MultichainProvider chainId={ chainData?.id }>
      <SocketProvider url={ getSocketUrl(chainData?.app_config) }>
        <Token/>
      </SocketProvider>
    </MultichainProvider>
  );
};

export default React.memo(MultichainToken);
