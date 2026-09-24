import { Grid, Box } from '@chakra-ui/react';
import type { UseQueryResult } from '@tanstack/react-query';
import React, { useCallback } from 'react';
import type { MouseEvent } from 'react';

import type { MarketplaceApp } from 'types/client/marketplace';

import useLazyRenderedList from 'lib/hooks/useLazyRenderedList';
import * as mixpanel from 'lib/mixpanel/index';

import EmptySearchResult from './EmptySearchResult';
import MarketplaceAppCard from './MarketplaceAppCard';

type Props = {
  apps: Array<MarketplaceApp>;
  showAppInfo: (id: string) => void;
  favoriteApps: Array<string>;
  onFavoriteClick: (id: string, isFavorite: boolean, source: 'Discovery view') => void;
  isLoading: boolean;
  selectedCategoryId?: string;
  onAppClick: (event: MouseEvent, id: string) => void;
  graphLinksQuery: UseQueryResult<Record<string, Array<{ title: string; url: string }>>, unknown>;
};

const MarketplaceList = ({
  apps, showAppInfo, favoriteApps, onFavoriteClick, isLoading, selectedCategoryId,
  onAppClick, graphLinksQuery,
}: Props) => {
  const { cutRef, renderedItemsNum } = useLazyRenderedList(apps, !isLoading, 16);

  const handleInfoClick = useCallback((id: string) => {
    mixpanel.logEvent(mixpanel.EventTypes.PAGE_WIDGET, { Type: 'More button', Info: id, Source: 'Discovery view' });
    showAppInfo(id);
  }, [ showAppInfo ]);

  const handleFavoriteClick = useCallback((id: string, isFavorite: boolean) => {
    onFavoriteClick(id, isFavorite, 'Discovery view');
  }, [ onFavoriteClick ]);

  return apps.length > 0 ? (
    <>
      <Grid
        templateColumns={{ md: 'repeat(auto-fill, minmax(270px, 1fr))' }}
        autoRows="1fr"
        gap={{ base: '16px', md: '24px' }}
        marginTop={{ base: 0, lg: 3 }}
      >
        { apps.slice(0, renderedItemsNum).map((app, index) => (
          <MarketplaceAppCard
            key={ app.id + (isLoading ? index : '') }
            onInfoClick={ handleInfoClick }
            id={ app.id }
            external={ app.external }
            url={ app.url }
            title={ app.title }
            description={ app.description }
            author={ app.author }
            logo={ app.logo }
            logoDarkMode={ app.logoDarkMode }
            shortDescription={ app.shortDescription }
            categories={ app.categories }
            isFavorite={ favoriteApps.includes(app.id) }
            onFavoriteClick={ handleFavoriteClick }
            isLoading={ isLoading }
            internalWallet={ app.internalWallet }
            onAppClick={ onAppClick }
            rating={ app.rating }
            ratingsTotalCount={ app.ratingsTotalCount }
            userRating={ app.userRating }
            graphLinks={ graphLinksQuery.data?.[app.id] }
          />
        )) }
      </Grid>
      <Box ref={ cutRef } h={ 0 }/>
    </>
  ) : (
    <EmptySearchResult selectedCategoryId={ selectedCategoryId } favoriteApps={ favoriteApps }/>
  );
};

export default React.memo(MarketplaceList);
