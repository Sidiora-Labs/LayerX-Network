import type { StackProps } from '@chakra-ui/react';
import { HStack } from '@chakra-ui/react';
import { useQuery } from '@tanstack/react-query';
import { shuffle } from 'es-toolkit';
import React from 'react';

import type { HighlightsBannerConfig } from 'types/homepage';

import config from 'configs/app';
import useFetch from 'lib/hooks/useFetch';
import { HOMEPAGE_HIGHLIGHTS_BANNER } from 'stubs/homepage';

import HighlightsItem from './highlights/HighlightsItem';

const HIGHLIGHTS_BANNER_COUNT = 3;

const Highlights = (props: StackProps) => {
  const fetch = useFetch();

  const { isPlaceholderData, data } = useQuery({
    queryKey: [ 'homepage-highlights' ],
    queryFn: async() => fetch(config.UI.homepage.highlights || '', undefined, { resource: 'homepage-highlights' }) as Promise<Array<HighlightsBannerConfig>>,
    select: (data) => {
      const pinnedBanners = data.filter((banner) => banner.is_pinned);
      const otherBanners = data.filter((banner) => !banner.is_pinned);

      return [
        ...pinnedBanners,
        ...shuffle(otherBanners),
      ].slice(0, HIGHLIGHTS_BANNER_COUNT);
    },
    enabled: Boolean(config.UI.homepage.highlights),
    staleTime: Infinity,
    placeholderData: Array(HIGHLIGHTS_BANNER_COUNT).fill(HOMEPAGE_HIGHLIGHTS_BANNER),
  });

  return (
    <HStack gap={ 3 } { ...props }>
      { data?.map((banner, index) => (
        <HighlightsItem key={ index } data={ banner } isLoading={ isPlaceholderData } totalNum={ data.length }/>
      )) }
    </HStack>
  );
};

export default React.memo(Highlights);
