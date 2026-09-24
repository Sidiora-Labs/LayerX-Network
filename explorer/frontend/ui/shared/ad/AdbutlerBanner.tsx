import { Flex, chakra } from '@chakra-ui/react';
import { useRouter } from 'next/navigation';
import Script from 'next/script';
import React from 'react';

import type { BannerProps } from './types';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import { isBrowser } from 'toolkit/utils/isBrowser';
import { connectAdbutler, placeAd, ADBUTLER_ACCOUNT } from 'ui/shared/ad/adbutlerScript';

import {
  DESKTOP_BANNER_WIDTH,
  DESKTOP_BANNER_HEIGHT,
  MOBILE_BANNER_WIDTH,
  MOBILE_BANNER_HEIGHT,
} from './consts';

const feature = config.features.adsBanner;

const AdbutlerBanner = ({ className, format = 'responsive' }: BannerProps) => {
  const router = useRouter();

  const isMobileViewport = useIsMobile();
  const isMobile = format === 'mobile' || (format === 'responsive' && isMobileViewport);

  React.useEffect(() => {
    if (!('adButler' in feature)) {
      return;
    }

    if (isBrowser() && window.AdButler) {
      const abkw = window.abkw || '';
      if (!window.AdButler.ads) {
        window.AdButler.ads = [];
      }
      const adButlerConfig = isMobile ? feature.adButler.config.mobile : feature.adButler.config.desktop;

      // @ts-ignore:
      let plc = window[`plc${ adButlerConfig.id }`] || 0;
      const banner = document.getElementById('ad-banner');
      if (banner) {
        banner.innerHTML = '<' + 'div id="placement_' + adButlerConfig?.id + '_' + plc + '"></' + 'div>';
      }

      // @ts-ignore:
      window.AdButler.ads.push({ handler: function(opt) {
        window.AdButler.register(
          ADBUTLER_ACCOUNT,
          adButlerConfig.id,
          [ adButlerConfig.width, adButlerConfig.height ],
          `placement_${ adButlerConfig.id }_` + opt.place,
          opt,
        );
      }, opt: { place: plc++, keywords: abkw, domain: 'servedbyadbutler.com', click: 'CLICK_MACRO_PLACEHOLDER' } });
    }
  }, [ router, isMobile ]);

  if (!('adButler' in feature)) {
    return null;
  }

  const getElementId = (id: string) => id + (format ? `_${ format }` : '');

  return (
    <Flex
      className={ className }
      id={ getElementId('adBanner') }
      h={ isMobile ? `${ MOBILE_BANNER_HEIGHT }px` : `${ DESKTOP_BANNER_HEIGHT }px` }
      w={ isMobile ? `${ MOBILE_BANNER_WIDTH }px` : `${ DESKTOP_BANNER_WIDTH }px` }
    >
      <Script strategy="lazyOnload" id="ad-butler-1">{ connectAdbutler }</Script>
      <Script strategy="lazyOnload" id="ad-butler-2">{ placeAd(isMobile) }</Script>
      <div id="ad-banner"></div>
    </Flex>
  );
};

export default chakra(AdbutlerBanner);
