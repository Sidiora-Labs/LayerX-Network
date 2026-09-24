import React from 'react';
import { WagmiProvider as WagmiProviderCore } from 'wagmi';

import wagmiConfig from 'lib/web3/wagmiConfig';

interface Props {
  children: React.ReactNode;
}

const WagmiProvider = ({ children }: Props) => {
  return (
    <WagmiProviderCore config={ wagmiConfig.config }>
      { children }
    </WagmiProviderCore>
  );
};

export default React.memo(WagmiProvider);
