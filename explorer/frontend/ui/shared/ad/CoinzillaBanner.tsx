import { Flex, chakra } from '@chakra-ui/react';
import Script from 'next/script';
import React from 'react';

import type { BannerProps } from './types';

import useIsMobile from 'lib/hooks/useIsMobile';
import { isBrowser } from 'toolkit/utils/isBrowser';

import {
  DESKTOP_BANNER_WIDTH,
  MOBILE_BANNER_WIDTH,
  DESKTOP_BANNER_HEIGHT,
  MOBILE_BANNER_HEIGHT,
} from './consts';

const CoinzillaBanner = ({ className, format = 'responsive' }: BannerProps) => {
  const isInBrowser = isBrowser();
  const isMobileViewport = useIsMobile();
  const isMobile = format === 'mobile' || (format === 'responsive' && isMobileViewport);

  const { width, height } = (() => {
    if (isMobile) {
      return { width: MOBILE_BANNER_WIDTH, height: MOBILE_BANNER_HEIGHT };
    }
    return { width: DESKTOP_BANNER_WIDTH, height: DESKTOP_BANNER_HEIGHT };
  })();

  React.useEffect(() => {
    if (isInBrowser) {
      window.coinzilla_display = window.coinzilla_display || [];
      const cDisplayPreferences = {
        zone: '26660bf627543e46851',
        width: width.toString(),
        height: height.toString(),
      };
      window.coinzilla_display.push(cDisplayPreferences);
    }
  }, [ isInBrowser, width, height ]);

  return (
    <Flex
      className={ className }
      id={ 'adBanner' + (format ? `_${ format }` : '') }
      h={ height ? `${ height }px` : { base: `${ MOBILE_BANNER_HEIGHT }px`, lg: `${ DESKTOP_BANNER_HEIGHT }px` } }
      w={ width ? `${ width }px` : undefined }
    >
      <Script strategy="lazyOnload" src="https://coinzillatag.com/lib/display.js"/>
      <div className="coinzilla" data-zone="C-26660bf627543e46851"></div>
    </Flex>
  );
};

export default chakra(CoinzillaBanner);
