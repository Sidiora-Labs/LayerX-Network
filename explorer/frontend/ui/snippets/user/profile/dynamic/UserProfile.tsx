import { type ButtonProps } from '@chakra-ui/react';
import { DynamicConnectButton, DynamicUserProfile, useDynamicContext, useIsLoggedIn } from '@dynamic-labs/sdk-react-core';
import React from 'react';

import { useMarketplaceContext } from 'lib/contexts/marketplace';
import useWalletDynamic from 'lib/web3/wallet/useWalletDynamic';

import UserProfileButton from './UserProfileButton';
import UserProfilePopover from './UserProfilePopover';

interface Props {
  buttonSize?: ButtonProps['size'];
  buttonVariant?: ButtonProps['variant'];
}

const UserProfile = ({ buttonSize, buttonVariant = 'header' }: Props) => {
  const isLoggedIn = useIsLoggedIn();
  const { user, authMode } = useDynamicContext();
  const wallet = useWalletDynamic({ source: isLoggedIn ? 'Profile dropdown' : 'Login' });
  const { isAutoConnectDisabled } = useMarketplaceContext();

  const handleOpenModal = React.useCallback(() => {
    wallet.openModal();
  }, [ wallet ]);

  if (isLoggedIn && authMode === 'connect-and-sign') {
    return (
      <>
        <UserProfileButton
          email={ user?.email }
          size={ buttonSize }
          variant={ buttonVariant }
          onClick={ handleOpenModal }
          selected
        />
        <DynamicUserProfile variant="modal"/>
      </>
    );
  }

  if (wallet.isConnected) {
    return (
      <>
        <UserProfilePopover isAutoConnectDisabled={ isAutoConnectDisabled }>
          <UserProfileButton
            email={ user?.email }
            size={ buttonSize }
            variant={ buttonVariant }
            selected
          />
        </UserProfilePopover>
        <DynamicUserProfile variant="modal"/>
      </>
    );
  }

  return (
    <DynamicConnectButton>
      <UserProfileButton
        as="div"
        size={ buttonSize }
        variant={ buttonVariant }
      />
    </DynamicConnectButton>
  );
};

export default React.memo(UserProfile);
