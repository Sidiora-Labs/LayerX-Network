import type { GetServerSidePropsContext, GetServerSidePropsResult } from 'next';

import type { AdBannerProviders } from 'types/client/adProviders';

import type { Route } from 'nextjs-routes';

import config from 'configs/app';
import * as cookies from 'lib/cookies';
import type * as metadata from 'lib/metadata';

import { isLikelyHumanBrowser, isKnownBotRequest } from '../utils/checkRealBrowser';

const adBannerFeature = config.features.adsBanner;

export interface Props<Pathname extends Route['pathname'] = never> {
  query: Route['query'];
  cookies: string;
  referrer: string;
  adBannerProvider: AdBannerProviders | null;
  // if apiData is undefined, Next.js will complain that it is not serializable
  // so we force it to be always present in the props but it can be null
  apiData: metadata.ApiData<Pathname> | null;
  uuid: string;
}

export const base = async <Pathname extends Route['pathname'] = never>({ req, res, query }: GetServerSidePropsContext):
Promise<GetServerSidePropsResult<Props<Pathname>>> => {
  const appProfile = req.headers?.['x-app-profile'] || cookies.getFromCookieString(req.headers.cookie || '', cookies.NAMES.APP_PROFILE);
  const adBannerProvider = (() => {
    if (adBannerFeature.isEnabled) {
      if ('additionalProvider' in adBannerFeature && adBannerFeature.additionalProvider) {
        // we need to get a random ad provider on the server side to keep it consistent with the client side
        const randomIndex = Math.round(Math.random());
        return [ adBannerFeature.provider, adBannerFeature.additionalProvider ][randomIndex];
      } else {
        return adBannerFeature.provider;
      }
    }
    return null;
  })();

  let uuid = cookies.getFromCookieString(req.headers.cookie || '', cookies.NAMES.UUID);
  if (!uuid && appProfile !== 'private') {
    uuid = crypto.randomUUID();
    res.setHeader('Set-Cookie', `${ cookies.NAMES.UUID }=${ uuid }; Path=/${ config.app.protocol === 'https' ? '; Secure' : '' }`);
  }

  const isTrackingDisabled = process.env.DISABLE_TRACKING === 'true' || appProfile === 'private';

  if (!isTrackingDisabled) {
    const isRealUser = isLikelyHumanBrowser(req) && !isKnownBotRequest(req);
    if (isRealUser) {
    // log pageview
      const hostname = req.headers.host;
      const timestamp = new Date().toISOString();
      const chainId = process.env.NEXT_PUBLIC_NETWORK_ID;
      const chainName = process.env.NEXT_PUBLIC_NETWORK_NAME;
      const publicRPC = process.env.NEXT_PUBLIC_NETWORK_RPC_URL;

      fetch('https://monitor.blockscout.com/count', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          hostname,
          timestamp,
          chainId,
          chainName,
          publicRPC,
          uuid,
        }),
      });
    }
  }

  return {
    props: {
      query,
      cookies: req.headers.cookie || '',
      referrer: req.headers.referer || '',
      adBannerProvider: adBannerProvider,
      apiData: null,
      uuid,
    },
  };
};
