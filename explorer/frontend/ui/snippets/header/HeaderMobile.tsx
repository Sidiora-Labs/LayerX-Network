import { Box, Flex } from '@chakra-ui/react';
import dynamic from 'next/dynamic';
import React from 'react';

import config from 'configs/app';
import { useIsSticky } from 'toolkit/hooks/useIsSticky';
import RewardsButton from 'ui/rewards/RewardsButton';
import NetworkIcon from 'ui/snippets/networkLogo/NetworkIcon';
import UserProfileAuth0 from 'ui/snippets/user/profile/auth0/UserProfileMobile';
import UserWalletMobile from 'ui/snippets/user/wallet/UserWalletMobile';

import RollupStageBadge from '../navigation/RollupStageBadge';
import TestnetBadge from '../navigation/TestnetBadge';
import SearchBarMobile from '../searchBar/SearchBarMobile';
import Burger from './Burger';

const UserProfileDynamic = dynamic(() => import('ui/snippets/user/profile/dynamic/UserProfile'), { ssr: false });

type Props = {
  hideSearchButton?: boolean;
  onGoToSearchResults?: (searchTerm: string) => void;
};

const HeaderMobile = ({ hideSearchButton, onGoToSearchResults }: Props) => {
  const ref = React.useRef<HTMLDivElement>(null);
  const isSticky = useIsSticky(ref, 5);

  const userProfile = (() => {
    const accountFeature = config.features.account;
    if (accountFeature.isEnabled) {
      switch (accountFeature.authProvider) {
        case 'auth0':
          return <UserProfileAuth0/>;
        case 'dynamic':
          return <UserProfileDynamic/>;
        default:
          return null;
      }
    }
    if (config.features.blockchainInteraction.isEnabled) {
      return <UserWalletMobile/>;
    }
  })();

  return (
    <Box
      ref={ ref }
      bgColor="bg.primary"
      display={{ base: 'block', lg: 'none' }}
      position="sticky"
      top="-1px"
      left={ 0 }
      zIndex="sticky2"
      pt="1px"
      height="56px"
    >
      <Flex
        as="header"
        paddingX={ 3 }
        paddingY={ 2 }
        bgColor="bg.primary"
        width="100%"
        alignItems="center"
        transitionProperty="box-shadow"
        transitionDuration="slow"
        boxShadow={ isSticky ? 'md' : 'none' }
      >
        <Burger/>
        <Flex alignItems="center" flexGrow={ 1 } mx={ 2 }>
          <NetworkIcon/>
          <TestnetBadge ml={ 2 }/>
          <RollupStageBadge ml={ 2 }/>
        </Flex>
        <Flex columnGap={ 2 }>
          { !hideSearchButton && <SearchBarMobile onGoToSearchResults={ onGoToSearchResults }/> }
          { config.features.rewards.isEnabled && <RewardsButton/> }
          { userProfile }
        </Flex>
      </Flex>
    </Box>
  );
};

export default React.memo(HeaderMobile);
