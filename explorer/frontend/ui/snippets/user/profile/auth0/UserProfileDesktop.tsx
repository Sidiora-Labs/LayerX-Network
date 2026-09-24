import { type ButtonProps } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type { Screen } from 'ui/snippets/auth/types';

import config from 'configs/app';
import * as mixpanel from 'lib/mixpanel';
import useAccount from 'lib/web3/useAccount';
import { PopoverBody, PopoverContent, PopoverRoot, PopoverTrigger } from 'toolkit/chakra/popover';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import AuthModal from 'ui/snippets/auth/AuthModal';
import useProfileQuery from 'ui/snippets/auth/useProfileQuery';

import UserProfileButton from './UserProfileButton';
import UserProfileContent from './UserProfileContent';

interface Props {
  buttonSize?: ButtonProps['size'];
  buttonVariant?: ButtonProps['variant'];
}

const initialScreen = {
  type: config.features.blockchainInteraction.isEnabled ? 'select_method' as const : 'email' as const,
};

const UserProfileDesktop = ({ buttonSize, buttonVariant = 'header' }: Props) => {
  const [ authInitialScreen, setAuthInitialScreen ] = React.useState<Screen>(initialScreen);
  const router = useRouter();

  const authModal = useDisclosure();
  const profileMenu = useDisclosure();

  const profileQuery = useProfileQuery();
  const { address: web3Address } = useAccount();

  const handleProfileButtonClick = React.useCallback(() => {
    if (profileQuery.data || web3Address) {
      mixpanel.logEvent(mixpanel.EventTypes.ACCOUNT_ACCESS, { Action: 'Dropdown open' });
      profileMenu.onOpen();
      return;
    }

    if (router.pathname === '/apps/[id]' && config.features.blockchainInteraction.isEnabled) {
      setAuthInitialScreen({ type: 'connect_wallet', loginToRewards: true });
    }

    authModal.onOpen();
  }, [ profileQuery.data, router.pathname, authModal, profileMenu, web3Address ]);

  const handleAddEmailClick = React.useCallback(() => {
    setAuthInitialScreen({ type: 'email', isAuth: true });
    authModal.onOpen();
    profileMenu.onClose();
  }, [ authModal, profileMenu ]);

  const handleAddAddressClick = React.useCallback(() => {
    setAuthInitialScreen({ type: 'connect_wallet', isAuth: true, loginToRewards: true });
    authModal.onOpen();
    profileMenu.onClose();
  }, [ authModal, profileMenu ]);

  const handleAuthModalClose = React.useCallback(() => {
    setAuthInitialScreen(initialScreen);
    authModal.onClose();
  }, [ authModal ]);

  const handleLoginClick = React.useCallback(() => {
    authModal.onOpen();
    profileMenu.onClose();
  }, [ authModal, profileMenu ]);

  const handleProfileMenuOpenChange = React.useCallback(({ open }: { open: boolean }) => {
    !open && profileMenu.onOpenChange({ open });
  }, [ profileMenu ]);

  return (
    <>
      <PopoverRoot positioning={{ placement: 'bottom-end' }} open={ profileMenu.open } onOpenChange={ handleProfileMenuOpenChange }>
        <PopoverTrigger>
          <UserProfileButton
            profileQuery={ profileQuery }
            size={ buttonSize }
            variant={ buttonVariant }
            onClick={ handleProfileButtonClick }
          />
        </PopoverTrigger>
        { (profileQuery.data || web3Address) && profileMenu.open && (
          <PopoverContent w="280px">
            <PopoverBody>
              <UserProfileContent
                data={ profileQuery.data }
                onClose={ profileMenu.onClose }
                onLogin={ handleLoginClick }
                onAddEmail={ handleAddEmailClick }
                onAddAddress={ handleAddAddressClick }
              />
            </PopoverBody>
          </PopoverContent>
        ) }
      </PopoverRoot>
      { authModal.open && (
        <AuthModal
          onClose={ handleAuthModalClose }
          initialScreen={ authInitialScreen }
        />
      ) }
    </>
  );
};

export default React.memo(UserProfileDesktop);
