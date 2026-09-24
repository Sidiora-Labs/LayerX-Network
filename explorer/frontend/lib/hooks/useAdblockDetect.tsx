import { useEffect } from 'react';

import type { AdBannerProviders } from 'types/client/adProviders';

import config from 'configs/app';
import { useAppContext } from 'lib/contexts/app';
import * as cookies from 'lib/cookies';
import { isBrowser } from 'toolkit/utils/isBrowser';

const DEFAULT_URL = 'https://request-global.czilladx.com';

// in general, detect should work with any ad-provider url (that is alive)
// but we see some false-positive results in certain browsers
const TEST_URLS: Record<AdBannerProviders, string> = {
  slise: 'https://v1.slise.xyz/serve',
  coinzilla: 'https://request-global.czilladx.com',
  adbutler: 'https://servedbyadbutler.com/app.js',
  none: DEFAULT_URL,
};

const feature = config.features.adsBanner;

export default function useAdblockDetect() {
  const hasAdblockCookie = cookies.get(cookies.NAMES.ADBLOCK_DETECTED, useAppContext().cookies);
  const provider = feature.isEnabled && feature.provider;

  useEffect(() => {
    if (isBrowser() && !hasAdblockCookie && provider) {
      const url = TEST_URLS[provider] || DEFAULT_URL;
      fetch(url, {
        method: 'HEAD',
        mode: 'no-cors',
        cache: 'no-store',
      })
        .then(() => {
          cookies.set(cookies.NAMES.ADBLOCK_DETECTED, 'false', { expires: 1 });
        })
        .catch(() => {
          cookies.set(cookies.NAMES.ADBLOCK_DETECTED, 'true', { expires: 1 });
        });
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}
