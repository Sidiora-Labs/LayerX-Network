import { useRouter } from 'next/router';
import React, { useCallback } from 'react';

import type { Route } from 'nextjs-routes';

import config from 'configs/app';
import { useRewardsContext } from 'lib/contexts/rewards';

import NavLink from './NavLink';

type Props = {
  isCollapsed?: boolean;
  onClick?: () => void;
};

const NavLinkRewards = ({ isCollapsed, onClick }: Props) => {
  const router = useRouter();
  const { openLoginModal, dailyRewardQuery, isAuth, isInitialized } = useRewardsContext();

  const pathname = '/account/merits';
  const nextRoute = { pathname } as Route;

  const handleClick = useCallback((e: React.MouseEvent) => {
    if (isInitialized && !isAuth) {
      e.preventDefault();
      openLoginModal();
    }
    onClick?.();
  }, [ onClick, isInitialized, isAuth, openLoginModal ]);

  if (!config.features.rewards.isEnabled) {
    return null;
  }

  return (
    <NavLink
      item={{
        text: 'Merits',
        icon: dailyRewardQuery.data?.available ? 'navigation/merits_with_dot' : 'navigation/merits',
        nextRoute: nextRoute,
        isActive: router.pathname === pathname,
      }}
      onClick={ handleClick }
      isCollapsed={ isCollapsed }
      isDisabled={ !isInitialized }
    />
  );
};

export default React.memo(NavLinkRewards);
