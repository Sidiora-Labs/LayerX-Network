import { Flex, chakra } from '@chakra-ui/react';
import { SliseAd } from '@slise/embed-react';
import React from 'react';

import type { BannerProps } from './types';

import config from 'configs/app';

import {
  DESKTOP_BANNER_HEIGHT,
  DESKTOP_BANNER_WIDTH,
  MOBILE_BANNER_HEIGHT,
  MOBILE_BANNER_WIDTH,
} from './consts';

const SliseBanner = ({ className, format = 'responsive' }: BannerProps) => {

  if (format === 'desktop') {
    return (
      <Flex className={ className } h={ `${ DESKTOP_BANNER_HEIGHT }px` }>
        <SliseAd
          slotId={ config.chain.name || '' }
          pub="pub-10"
          format="728x90"
          style={{ width: `${ DESKTOP_BANNER_WIDTH }px`, height: `${ DESKTOP_BANNER_HEIGHT }px` }}/>
      </Flex>
    );
  }

  if (format === 'mobile') {
    return (
      <Flex
        className={ className }
        h={ `${ MOBILE_BANNER_HEIGHT }px` }
        w={ `${ MOBILE_BANNER_WIDTH }px` }
        justifyContent="center"
      >
        <SliseAd
          slotId={ config.chain.name || '' }
          pub="pub-10"
          format="320x100"
          style={{ width: `${ MOBILE_BANNER_WIDTH }px`, height: `${ MOBILE_BANNER_HEIGHT }px` }}/>
      </Flex>
    );
  }

  return (
    <>
      <Flex className={ className } h={ `${ DESKTOP_BANNER_HEIGHT }px` } display={{ base: 'none', lg: 'flex' }}>
        <SliseAd
          slotId={ config.chain.name || '' }
          pub="pub-10"
          format="728x90"
          style={{ width: `${ DESKTOP_BANNER_WIDTH }px`, height: `${ DESKTOP_BANNER_HEIGHT }px` }}/>
      </Flex>
      <Flex
        className={ className }
        h={ `${ MOBILE_BANNER_HEIGHT }px` }
        w={ `${ MOBILE_BANNER_WIDTH }px` }
        justifyContent="center"
        display={{ base: 'flex', lg: 'none' }}
      >
        <SliseAd
          slotId={ config.chain.name || '' }
          pub="pub-10"
          format="320x100"
          style={{ width: `${ MOBILE_BANNER_WIDTH }px`, height: `${ MOBILE_BANNER_HEIGHT }px` }}/>
      </Flex>
    </>
  );
};

export default chakra(SliseBanner);
