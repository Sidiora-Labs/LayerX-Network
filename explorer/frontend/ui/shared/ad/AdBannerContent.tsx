import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { BannerFormat } from './types';
import type { AdBannerProviders } from 'types/client/adProviders';

import config from 'configs/app';
import useAccount from 'lib/web3/useAccount';
import { Skeleton } from 'toolkit/chakra/skeleton';
import useProfileQuery from 'ui/snippets/auth/useProfileQuery';

import AdbutlerBanner from './AdbutlerBanner';
import CoinzillaBanner from './CoinzillaBanner';
import { DESKTOP_BANNER_WIDTH, MOBILE_BANNER_WIDTH } from './consts';
import SliseBanner from './SliseBanner';
import SpecifyBanner from './SpecifyBanner';

const feature = config.features.adsBanner;
const isSpecifyEnabled = feature.isEnabled && feature.isSpecifyEnabled;

interface Props {
  className?: string;
  isLoading?: boolean;
  format?: BannerFormat;
  provider: AdBannerProviders;
}

const AdBannerContent = ({ className, isLoading, provider, format }: Props) => {
  const { address: addressWC, isConnecting } = useAccount();
  const profileQuery = useProfileQuery();
  const [ showSpecify, setShowSpecify ] = React.useState(isSpecifyEnabled);

  const handleEmptySpecify = React.useCallback(() => {
    setShowSpecify(false);
  }, []);

  const address = addressWC || profileQuery.data?.address_hash as `0x${ string }` | undefined;

  const content = (() => {
    if (showSpecify) {
      const isLoading = address ? false : profileQuery.isLoading || isConnecting;
      return <SpecifyBanner format={ format } address={ address } onEmpty={ handleEmptySpecify } isLoading={ isLoading }/>;
    }
    switch (provider) {
      case 'adbutler':
        return <AdbutlerBanner format={ format }/>;
      case 'coinzilla':
        return <CoinzillaBanner format={ format }/>;
      case 'slise':
        return <SliseBanner format={ format }/>;
    }
  })();

  return (
    <Skeleton
      className={ className }
      loading={ isLoading }
      borderRadius="none"
      maxW={{ base: `${ MOBILE_BANNER_WIDTH }px`, lg: `${ DESKTOP_BANNER_WIDTH }px` }}
      w="100%"
    >
      { content }
    </Skeleton>
  );
};

export default chakra(AdBannerContent);
