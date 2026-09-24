import { Flex, Separator, Box, HStack } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import { useAppContext } from 'lib/contexts/app';
import * as cookies from 'lib/cookies';
import useIsMobile from 'lib/hooks/useIsMobile';
import useProvider from 'lib/web3/useProvider';
import { CONTENT_MAX_WIDTH } from 'ui/shared/layout/utils';
import NetworkAddToWallet from 'ui/shared/NetworkAddToWallet';

import DeFiDropdown from './DeFiDropdown';
import NetworkMenu from './NetworkMenu';
import Settings from './settings/Settings';
import TopBarStats from './TopBarStats';

const TopBar = () => {
  const hideAddToWalletButtonCookie = cookies.get(cookies.NAMES.HIDE_ADD_TO_WALLET_BUTTON, useAppContext().cookies);
  const [ isAddChainButtonVisible, setIsAddChainButtonVisible ] = React.useState(hideAddToWalletButtonCookie !== 'topbar');

  const web3 = useProvider();
  const isMobile = useIsMobile();

  const hasAddChainButton = Boolean(
    isAddChainButtonVisible &&
    web3.data?.provider &&
    web3.data?.wallet &&
    config.chain.rpcUrls.length &&
    config.features.web3Wallet.isEnabled &&
    !config.features.multichain.isEnabled &&
    !isMobile,
  );
  const hasDeFiDropdown = Boolean(config.features.deFiDropdown.isEnabled);

  const handleAddSuccess = React.useCallback(() => {
    cookies.set(cookies.NAMES.HIDE_ADD_TO_WALLET_BUTTON, 'topbar', { expires: 3 * 365 });
    setIsAddChainButtonVisible(false);
  }, [ ]);

  return (
    // not ideal if scrollbar is visible, but better than having a horizontal scroll
    <Box bgColor={{ _light: 'theme.topbar.bg._light', _dark: 'theme.topbar.bg._dark' }} position="sticky" left={ 0 } width="100%" maxWidth="100vw">
      <Flex
        py={ 2 }
        px={{ base: 3, lg: 6 }}
        m="0 auto"
        justifyContent="space-between"
        alignItems="center"
        maxW={ `${ CONTENT_MAX_WIDTH }px` }
      >
        <HStack gap={ 0 } fontSize="xs">
          { Boolean(config.UI.featuredNetworks.items || config.features.multichain.isEnabled) && <NetworkMenu/> }
          { !config.features.multichain.isEnabled ? <TopBarStats/> : <div/> }
        </HStack>
        <HStack
          alignItems="center"
          separator={ <Separator mx={{ base: 2, lg: 3 }} height={ 4 }/> }
        >
          { (hasAddChainButton || hasDeFiDropdown) && (
            <HStack>
              { hasAddChainButton && <NetworkAddToWallet source="Top bar" onAddSuccess={ handleAddSuccess }/> }
              { hasDeFiDropdown && <DeFiDropdown/> }
            </HStack>
          ) }
          <Settings/>
        </HStack>
      </Flex>
    </Box>
  );
};

export default React.memo(TopBar);
