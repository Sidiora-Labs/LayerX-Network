import { chakra, Flex } from '@chakra-ui/react';
import React from 'react';

import type { MarketplaceApp } from 'types/client/marketplace';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import { useAppContext } from 'lib/contexts/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import * as mixpanel from 'lib/mixpanel/index';
import { Link } from 'toolkit/chakra/link';
import { BackToButton } from 'toolkit/components/buttons/BackToButton';
import { makePrettyLink } from 'toolkit/utils/url';
import RewardsButton from 'ui/rewards/RewardsButton';
import NetworkIcon from 'ui/snippets/networkLogo/NetworkIcon';
import UserProfileDesktop from 'ui/snippets/user/UserProfileDesktop';

import MarketplaceAppInfo from './MarketplaceAppInfo';
import Rating from './Rating/Rating';

type Props = {
  appId: string;
  data: MarketplaceApp | undefined;
  isLoading: boolean;
};

const MarketplaceAppTopBar = ({ appId, data, isLoading }: Props) => {
  const appProps = useAppContext();
  const isMobile = useIsMobile();

  const goBackUrl = React.useMemo(() => {
    if (appProps.referrer && appProps.referrer.includes('/apps') && !appProps.referrer.includes('/apps/')) {
      return appProps.referrer;
    }
    return route({ pathname: '/apps' });
  }, [ appProps.referrer ]);

  const handleBackToClick = React.useCallback(() => {
    mixpanel.logEvent(mixpanel.EventTypes.BUTTON_CLICK, { Content: 'Back to', Source: mixpanel.PAGE_TYPE_DICT['/apps/[id]'] });
  }, []);

  return (
    <Flex alignItems="center" mb={{ base: 3, md: 2 }} rowGap={ 3 } columnGap={ 2 }>
      { !isMobile && <NetworkIcon mr={ 4 }/> }
      <BackToButton
        href={ goBackUrl }
        hint="Back to dApps list"
        loading={ isLoading }
        onClick={ handleBackToClick }
      />
      <Link
        external
        href={ data?.url }
        variant="underlaid"
        textStyle="sm"
        minW={ 0 }
        maxW={{ base: 'calc(100% - 114px)', md: 'auto' }}
        display="flex"
        loading={ isLoading }
      >
        <chakra.span truncate>
          { makePrettyLink(data?.url)?.domain }
        </chakra.span>
      </Link>
      <MarketplaceAppInfo data={ data } isLoading={ isLoading }/>
      <Rating
        appId={ appId }
        rating={ data?.rating }
        ratingsTotalCount={ data?.ratingsTotalCount }
        userRating={ data?.userRating }
        isLoading={ isLoading }
        source="App page"
      />
      { !isMobile && (
        <Flex ml="auto" gap={ 2 }>
          { config.features.rewards.isEnabled && <RewardsButton size="sm"/> }
          <UserProfileDesktop buttonSize="sm"/>
        </Flex>
      ) }
    </Flex>
  );
};

export default MarketplaceAppTopBar;
