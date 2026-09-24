import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import type { Route } from 'nextjs-routes';

import config from 'configs/app';
import useApiFetch from 'lib/api/useApiFetch';
import { getResourceKey } from 'lib/api/useApiQuery';
import { useRewardsContext } from 'lib/contexts/rewards';
import * as cookies from 'lib/cookies';
import * as mixpanel from 'lib/mixpanel';
import { toaster } from 'toolkit/chakra/toaster';

const PROTECTED_ROUTES: Array<Route['pathname']> = [
  '/account/api-key',
  '/account/custom-abi',
  '/account/merits',
  '/account/tag-address',
  '/account/verified-addresses',
  '/account/watchlist',
  '/auth/profile',
];

export default function useLogout() {
  const router = useRouter();
  const queryClient = useQueryClient();
  const apiFetch = useApiFetch();
  const { logout: rewardsLogout } = useRewardsContext();

  return React.useCallback(async() => {
    try {
      await apiFetch('general:auth_logout');
      cookies.remove(cookies.NAMES.API_TOKEN);

      if (config.features.rewards.isEnabled) {
        rewardsLogout();
      }

      mixpanel.logEvent(mixpanel.EventTypes.ACCOUNT_ACCESS, { Action: 'Logged out' }, { send_immediately: true });
      mixpanel.reset();

      if (
        PROTECTED_ROUTES.includes(router.pathname) ||
          (router.pathname === '/txs' && router.query.tab === 'watchlist')
      ) {
        await router.push({ pathname: '/' }, undefined, { shallow: true });
      }

      queryClient.resetQueries({
        queryKey: getResourceKey('general:user_info'),
        exact: true,
      });
      queryClient.resetQueries({
        queryKey: getResourceKey('general:custom_abi'),
        exact: true,
      });
    } catch (error) {
      toaster.error({
        title: 'Logout failed',
        description: 'Please try again later',
      });
    }
  }, [ apiFetch, rewardsLogout, queryClient, router ]);
}
