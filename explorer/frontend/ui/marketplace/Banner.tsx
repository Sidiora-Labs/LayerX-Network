import { Flex } from '@chakra-ui/react';
import type { MouseEvent } from 'react';
import React from 'react';

import type { MarketplaceApp } from 'types/client/marketplace';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import { apps as appsMock } from 'mocks/apps/apps';
import AdBanner from 'ui/shared/ad/AdBanner';

import FeaturedApp from './Banner/FeaturedApp';
import IframeBanner from './Banner/IframeBanner';

const feature = config.features.marketplace;

type BannerProps = {
  apps: Array<MarketplaceApp> | undefined;
  favoriteApps: Array<string>;
  isLoading: boolean;
  onInfoClick: (id: string) => void;
  onFavoriteClick: (id: string, isFavorite: boolean, source: 'Banner') => void;
  onAppClick: (event: MouseEvent, id: string) => void;
};

const Banner = ({ apps = [], favoriteApps, isLoading, onInfoClick, onFavoriteClick, onAppClick }: BannerProps) => {
  const isMobile = useIsMobile();

  if (!feature.isEnabled) {
    return null;
  }

  let content = null;

  if (feature.featuredApp) {
    const app = apps.find(app => app.id === feature.featuredApp);
    const isFavorite = favoriteApps.includes(feature.featuredApp);
    if (!isLoading && !app) {
      return null;
    }
    content = (
      <FeaturedApp
        app={ app || appsMock[0] }
        isFavorite={ isFavorite }
        isLoading={ isLoading }
        onInfoClick={ onInfoClick }
        onFavoriteClick={ onFavoriteClick }
        onAppClick={ onAppClick }
      />
    );
  } else if (feature.banner) {
    content = <IframeBanner contentUrl={ feature.banner.contentUrl } linkUrl={ feature.banner.linkUrl }/>;
  }

  if (!content) {
    return null;
  }

  return (
    <Flex gap={ 6 }>
      { content }
      { !isMobile && (
        <AdBanner
          format="mobile"
          w="fit-content"
          flexShrink={ 0 }
          borderRadius="md"
          overflow="hidden"
        />
      ) }
    </Flex>
  );
};

export default Banner;
