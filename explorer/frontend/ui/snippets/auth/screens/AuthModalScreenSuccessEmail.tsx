import { chakra, Box, Text } from '@chakra-ui/react';
import React from 'react';

import type { Screen } from '../types';
import type { UserInfo } from 'types/api/account';

import config from 'configs/app';
import { Button } from 'toolkit/chakra/button';

interface Props {
  email: string;
  onConnectWallet: (screen: Screen) => void;
  onClose: () => void;
  isAuth?: boolean;
  profile: UserInfo | undefined;
}

const AuthModalScreenSuccessEmail = ({ email, onConnectWallet, onClose, isAuth, profile }: Props) => {
  const handleConnectWalletClick = React.useCallback(() => {
    onConnectWallet({ type: 'connect_wallet', isAuth: true, loginToRewards: true });
  }, [ onConnectWallet ]);

  if (isAuth) {
    return (
      <Box>
        <Text>
          Your account was linked to{ ' ' }
          <chakra.span fontWeight="700">{ email }</chakra.span>{ ' ' }
          email. Use for the next login.
        </Text>
        <Button
          mt={ 6 }
          variant="outline"
          onClick={ onClose }
        >
          Got it!
        </Button>
      </Box>
    );
  }

  return (
    <Box>
      <Text>
        <chakra.span fontWeight="700">{ email }</chakra.span>{ ' ' }
        email has been successfully used to log in to your Blockscout account.
      </Text>
      { !profile?.address_hash && config.features.blockchainInteraction.isEnabled ? (
        <>
          <Text mt={ 6 }>Add your web3 wallet to safely interact with smart contracts and dapps inside Blockscout.</Text>
          <Button mt={ 6 } onClick={ handleConnectWalletClick }>Connect wallet</Button>
        </>
      ) : (
        <Button
          variant="outline"
          mt={ 6 }
          onClick={ onClose }
        >
          Got it!
        </Button>
      ) }
    </Box>
  );
};

export default React.memo(AuthModalScreenSuccessEmail);
