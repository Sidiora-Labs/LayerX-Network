import { chakra, Box, Flex, Spinner } from '@chakra-ui/react';
import React from 'react';

import delay from 'lib/delay';
import useWeb3AccountWithDomain from 'lib/web3/useAccountWithDomain';
import useWeb3Wallet from 'lib/web3/useWallet';
import { Button } from 'toolkit/chakra/button';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Hint } from 'toolkit/components/Hint/Hint';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  onClose?: () => void;
  className?: string;
}

const UserProfileContentWallet = ({ onClose, className }: Props) => {
  const web3Wallet = useWeb3Wallet({ source: 'Profile dropdown' });

  const web3AccountWithDomain = useWeb3AccountWithDomain(true);

  const handleConnectWalletClick = React.useCallback(async() => {
    web3Wallet.openModal();
    await delay(300);
    onClose?.();
  }, [ web3Wallet, onClose ]);

  const handleOpenWalletClick = React.useCallback(async() => {
    web3Wallet.openModal();
    await delay(300);
    onClose?.();
  }, [ web3Wallet, onClose ]);

  const handleAddressClick = React.useCallback(() => {
    onClose?.();
  }, [ onClose ]);

  const content = (() => {
    if (web3Wallet.isConnected && web3AccountWithDomain.address) {
      return (
        <Flex
          alignItems="center"
          columnGap={ 2 }
          bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
          px={ 2 }
          py="10px"
          borderRadius="base"
          justifyContent="space-between"
        >
          <AddressEntity
            address={{ hash: web3AccountWithDomain.address, ens_domain_name: web3AccountWithDomain.domain }}
            isLoading={ web3AccountWithDomain.isLoading }
            truncation="dynamic"
            fontSize="sm"
            fontWeight={ 500 }
            noAltHash
            onClick={ handleAddressClick }
          />
          { web3Wallet.isReconnecting ? <Spinner size="sm" m="2px" flexShrink={ 0 }/> : (
            <IconButton
              aria-label="Open wallet"
              variant="icon_secondary"
              size="2xs"
              onClick={ handleOpenWalletClick }
              loading={ web3Wallet.isOpen }
            >
              <IconSvg name="gear"/>
            </IconButton>
          ) }
        </Flex>
      );
    }

    return (
      <Button
        size="sm"
        onClick={ handleConnectWalletClick }
        loading={ web3Wallet.isOpen }
        loadingText="Connect Wallet"
        w="100%"
      >
        Connect
      </Button>
    );
  })();

  return (
    <Box className={ className }>
      <Flex px={ 1 } mb="1" textStyle="xs" alignItems="center" fontWeight="500">
        <span>Connected wallet</span>
        <Hint
          label={
            web3Wallet.isConnected ?
              'This wallet is currently connected to Blockscout and used for interacting with apps and smart contracts' :
              'This wallet is used for interacting with apps and smart contracts'
          }
          boxSize={ 4 }
          ml={ 1 }
        />
      </Flex>
      { content }
    </Box>
  );
};

export default React.memo(chakra(UserProfileContentWallet));
