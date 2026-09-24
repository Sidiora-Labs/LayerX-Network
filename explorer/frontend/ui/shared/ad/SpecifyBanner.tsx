import { Box, chakra } from '@chakra-ui/react';
import type { SpecifyAd } from '@specify-sh/sdk';
import Specify, { ImageFormat } from '@specify-sh/sdk';
import React from 'react';

import type { BannerProps } from './types';

import useIsMobile from 'lib/hooks/useIsMobile';
import { Image } from 'toolkit/chakra/image';

import {
  DESKTOP_BANNER_HEIGHT,
  DESKTOP_BANNER_WIDTH,
  MOBILE_BANNER_WIDTH,
  MOBILE_BANNER_HEIGHT,
} from './consts';

const PUBLISHER_KEY = 'spk_uq51124ciii28vt00f8za4m5hibpuh';

type Props = BannerProps & {
  address: `0x${ string }` | undefined;
  onEmpty: () => void;
  isLoading: boolean;
};

const SpecifyBanner = ({ className, format = 'responsive', address, onEmpty, isLoading }: Props) => {
  const isMobileViewport = useIsMobile();
  const isMobile = format === 'mobile' || (format === 'responsive' && isMobileViewport);
  const [ ad, setAd ] = React.useState<SpecifyAd | null>(null);
  const [ isFetching, setIsFetching ] = React.useState(true);

  React.useEffect(() => {
    if (!isLoading) {
      const specify = new Specify({
        publisherKey: PUBLISHER_KEY,
        cacheMostRecentAddress: true,
      });
      const fetchContent = async() => {
        try {
          const content = await specify.serve(
            address ? [ address as `0x${ string }` ] : null,
            { imageFormat: isMobile ? ImageFormat.SHORT_BANNER : ImageFormat.LONG_BANNER },
          );
          if (content?.imageUrl) {
            setAd(content);
          } else {
            onEmpty();
          }
        } catch (error) {
          onEmpty();
        } finally {
          setIsFetching(false);
        }
      };

      fetchContent();
    }
  }, [ address, isMobile, onEmpty, isLoading ]);

  const handleClick = React.useCallback(() => {
    window.open(ad?.ctaUrl, '_blank', 'noopener,noreferrer');
  }, [ ad?.ctaUrl ]);

  const { width, height } = (() => {
    if (isMobile) {
      return { width: MOBILE_BANNER_WIDTH, height: MOBILE_BANNER_HEIGHT };
    }
    return { width: DESKTOP_BANNER_WIDTH, height: DESKTOP_BANNER_HEIGHT };
  })();

  if (isLoading || isFetching) {
    return (
      <Box className={ className }
        h={ height ? `${ height }px` : { base: `${ MOBILE_BANNER_HEIGHT }px`, lg: `${ DESKTOP_BANNER_HEIGHT }px` } }
        w={ width ? `${ width }px` : undefined }
      />
    );
  }

  if (!ad) return null;

  return (
    <Image
      src={ ad.imageUrl }
      alt={ ad.headline }
      cursor="pointer"
      onClick={ handleClick }
      className={ className }
      w={ width ? `${ width }px` : { base: `${ MOBILE_BANNER_WIDTH }px`, lg: `${ DESKTOP_BANNER_WIDTH }px` } }
      h={ height ? `${ height }px` : { base: `${ MOBILE_BANNER_HEIGHT }px`, lg: `${ DESKTOP_BANNER_HEIGHT }px` } }
    />
  );
};

export default chakra(SpecifyBanner);
