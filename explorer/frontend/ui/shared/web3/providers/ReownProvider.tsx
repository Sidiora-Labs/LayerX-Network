import type { AppKitNetwork } from '@reown/appkit/networks';
import { createAppKit, useAppKitTheme } from '@reown/appkit/react';
import React from 'react';

import config from 'configs/app';
import { chains } from 'lib/web3/chains';
import wagmiConfig from 'lib/web3/wagmiConfig';
import { useColorMode } from 'toolkit/chakra/color-mode';
import colors from 'toolkit/theme/foundations/colors';
import { BODY_TYPEFACE } from 'toolkit/theme/foundations/typography';
import zIndex from 'toolkit/theme/foundations/zIndex';

import WagmiProvider from './WagmiProvider';

const feature = config.features.blockchainInteraction;

const initReown = () => {
  try {
    if (!feature.isEnabled || !wagmiConfig.adapter || feature.connectorType === 'dynamic') {
      return;
    }

    createAppKit({
      adapters: [ wagmiConfig.adapter ],
      networks: chains as [AppKitNetwork, ...Array<AppKitNetwork>],
      metadata: {
        name: `${ config.chain.name } explorer`,
        description: `${ config.chain.name } explorer`,
        url: config.app.baseUrl,
        icons: [ config.UI.navigation.icon.default ].filter(Boolean),
      },
      projectId: feature.reown.projectId,
      features: {
        analytics: false,
        email: false,
        socials: [],
        onramp: false,
        swaps: false,
      },
      themeVariables: {
        '--w3m-font-family': `${ BODY_TYPEFACE }, sans-serif`,
        '--w3m-accent': colors.blue[600].value,
        '--w3m-border-radius-master': '2px',
        '--w3m-z-index': zIndex?.modal2?.value,
      },
      featuredWalletIds: feature.reown.featuredWalletIds,
      allowUnsupportedChain: true,
    });
  } catch (error) {}
};

initReown();

interface Props {
  children: React.ReactNode;
}

const ReownProvider = ({ children }: Props) => {
  const { colorMode } = useColorMode();
  const { setThemeMode } = useAppKitTheme();

  React.useEffect(() => {
    setThemeMode(colorMode ?? 'light');
  }, [ colorMode, setThemeMode ]);

  return (
    <WagmiProvider>
      { children }
    </WagmiProvider>
  );
};

export default React.memo(ReownProvider);
