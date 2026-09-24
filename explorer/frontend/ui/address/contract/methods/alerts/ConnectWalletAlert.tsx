import { Flex, Spinner } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import useWeb3Wallet from 'lib/web3/useWallet';
import { Alert } from 'toolkit/chakra/alert';
import { Button } from 'toolkit/chakra/button';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  isLoading?: boolean;
}

const ConnectWalletAlert = ({ isLoading }: Props) => {
  const web3Wallet = useWeb3Wallet({ source: 'Smart contracts' });
  const isMobile = useIsMobile();

  const handleOpenWalletClick = React.useCallback(() => {
    web3Wallet.openModal();
  }, [ web3Wallet ]);

  const content = (() => {
    if (!web3Wallet.isConnected) {
      return (
        <Flex alignItems="center" flexWrap="wrap" columnGap={ 3 } rowGap={ 2 }>
          <span>To interact with the contract, please</span>
          <Button
            onClick={ web3Wallet.connect }
            size="sm"
            loading={ web3Wallet.isOpen }
            loadingText="Connect your wallet"
          >
            Connect your wallet
          </Button>
        </Flex>
      );
    }

    return (
      <Flex alignItems="center" columnGap={ 2 }>
        <span>Connected to </span>
        <AddressEntity
          address={{ hash: web3Wallet.address || '' }}
          truncation={ isMobile ? 'constant' : 'dynamic' }
          fontWeight={ 600 }
          noAltHash
          noLink
          noCopy
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
  })();

  return (
    <Skeleton loading={ isLoading }>
      <Alert status="info" descriptionProps={{ alignItems: 'center' }}>
        { content }
      </Alert>
    </Skeleton>
  );
};

const Fallback = () => null;

export default config.features.blockchainInteraction.isEnabled ? ConnectWalletAlert : Fallback;
