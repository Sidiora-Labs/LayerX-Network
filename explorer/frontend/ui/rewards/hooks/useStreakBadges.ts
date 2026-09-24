import { useMemo } from 'react';

import type { GetAvailableBadgesResponse } from '@blockscout/points-types';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { useRewardsContext } from 'lib/contexts/rewards';

const feature = config.features.rewards;

export default function useStreakBadges() {
  const { isAuth, dailyRewardQuery } = useRewardsContext();

  const badgesQuery = useApiQuery<'rewards:user_badges', unknown, GetAvailableBadgesResponse>('rewards:user_badges', {
    queryOptions: {
      enabled: feature.isEnabled && isAuth,
      select: (data) => ({
        ...data,
        items: data.items
          .sort((a, b) => Number(a.requirements?.streak || 0) - Number(b.requirements?.streak || 0))
          .slice(0, 3), // UI limit
      }),
    },
  });

  const nextAchievementText = useMemo(() => {
    try {
      if (badgesQuery.isPending || dailyRewardQuery.isPending) {
        return 'Next achievement in N/A day'; // for skeleton
      }
      if (!badgesQuery.data?.items || !dailyRewardQuery.data?.streak) {
        return undefined;
      }
      const currentStreak = Number(dailyRewardQuery.data.streak);
      const next = badgesQuery.data.items.find((b) =>
        !(b.is_qualified || b.is_whitelisted || b.is_minted) &&
        Number(b.requirements?.streak) > currentStreak,
      );
      if (!next) {
        return 'All achievements are earned';
      }
      const target = Number(next.requirements?.streak || 0);
      const diff = target - currentStreak;
      return `Next achievement in ${ diff } day${ diff === 1 ? '' : 's' }`;
    } catch {
      return undefined;
    }
  }, [ badgesQuery, dailyRewardQuery ]);

  return {
    badgesQuery,
    nextAchievementText,
    isLoading: badgesQuery.isPending,
  };
}
