import { Text } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import type { EventTypes, EventPayload } from 'lib/mixpanel/index';
import type { PopoverContentProps } from 'toolkit/chakra/popover';
import { PopoverBody, PopoverContent, PopoverRoot } from 'toolkit/chakra/popover';
import { Rating as RatingComponent } from 'toolkit/chakra/rating';
import { Skeleton } from 'toolkit/chakra/skeleton';
import useIsAuth from 'ui/snippets/auth/useIsAuth';

import Content from './PopoverContent';
import TriggerButton from './TriggerButton';

const feature = config.features.marketplace;
const isEnabled = feature.isEnabled && 'api' in feature;

type Props = {
  appId: string;
  rating?: number;
  ratingsTotalCount?: number;
  userRating?: number;
  isLoading?: boolean;
  fullView?: boolean;
  source: EventPayload<EventTypes.APP_FEEDBACK>['Source'];
  popoverContentProps?: PopoverContentProps;
};

const Rating = ({
  appId, rating, ratingsTotalCount, userRating,
  isLoading, fullView, source, popoverContentProps,
}: Props) => {
  const isAuth = useIsAuth();

  if (!isEnabled) {
    return null;
  }

  return (
    <Skeleton
      display="flex"
      alignItems="center"
      loading={ isLoading }
      w={ (isLoading && !fullView) ? '40px' : 'auto' }
    >
      { fullView && (
        <>
          <RatingComponent defaultValue={ Math.floor(rating || 0) } readOnly key={ rating }/>
          { rating && <Text fontSize="md" ml={ 2 }>{ rating }</Text> }
          { ratingsTotalCount && <Text color="text.secondary" textStyle="md" ml={ 1 }>({ ratingsTotalCount })</Text> }
        </>
      ) }
      <PopoverRoot positioning={{ placement: 'bottom' }}>

        <TriggerButton
          rating={ rating }
          count={ ratingsTotalCount }
          fullView={ fullView }
          canRate={ isAuth }
        />
        { isAuth ? (
          <PopoverContent w="250px" { ...popoverContentProps }>
            <PopoverBody>
              <Content
                appId={ appId }
                userRating={ userRating }
                source={ source }
              />
            </PopoverBody>
          </PopoverContent>
        ) : <PopoverContent/> }
      </PopoverRoot>
    </Skeleton>
  );
};

export default Rating;
