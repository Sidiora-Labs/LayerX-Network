import { type ButtonProps } from '@chakra-ui/react';
import dynamic from 'next/dynamic';

import config from 'configs/app';
import UserProfileAuth0 from 'ui/snippets/user/profile/auth0/UserProfileDesktop';
import UserWalletDesktop from 'ui/snippets/user/wallet/UserWalletDesktop';

const UserProfileDynamic = dynamic(() => import('ui/snippets/user/profile/dynamic/UserProfile'), { ssr: false });

interface Props {
  buttonSize?: ButtonProps['size'];
  buttonVariant?: ButtonProps['variant'];
}

const UserProfileDesktop = ({ buttonSize, buttonVariant = 'header' }: Props) => {
  const accountFeature = config.features.account;
  if (accountFeature.isEnabled) {
    switch (accountFeature.authProvider) {
      case 'auth0':
        return <UserProfileAuth0 buttonSize={ buttonSize } buttonVariant={ buttonVariant }/>;
      case 'dynamic':
        return <UserProfileDynamic buttonSize={ buttonSize } buttonVariant={ buttonVariant }/>;
      default:
        return null;
    }
  }
  if (config.features.blockchainInteraction.isEnabled) {
    return <UserWalletDesktop buttonSize={ buttonSize } buttonVariant={ buttonVariant }/>;
  }
  return null;
};

export default UserProfileDesktop;
