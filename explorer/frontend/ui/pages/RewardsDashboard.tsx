import { Flex } from '@chakra-ui/react';
import React, { useEffect, useState } from 'react';

import config from 'configs/app';
import { useRewardsContext } from 'lib/contexts/rewards';
import useIsMobile from 'lib/hooks/useIsMobile';
import { Alert } from 'toolkit/chakra/alert';
import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import { apos } from 'toolkit/utils/htmlEntities';
import DailyRewardClaimButton from 'ui/rewards/dashboard/DailyRewardClaimButton';
import RewardsDashboardCard from 'ui/rewards/dashboard/RewardsDashboardCard';
import RewardsDashboardCardValue from 'ui/rewards/dashboard/RewardsDashboardCardValue';
import RewardsStreakModal from 'ui/rewards/dashboard/streakModal/RewardsStreakModal';
import ActivityTab from 'ui/rewards/dashboard/tabs/ActivityTab';
import ReferralsTab from 'ui/rewards/dashboard/tabs/ReferralsTab';
import ResourcesTab from 'ui/rewards/dashboard/tabs/ResourcesTab';
import useStreakBadges from 'ui/rewards/hooks/useStreakBadges';
import AdBanner from 'ui/shared/ad/AdBanner';
import PageTitle from 'ui/shared/Page/PageTitle';
import useRedirectForInvalidAuthToken from 'ui/snippets/auth/useRedirectForInvalidAuthToken';

const RewardsDashboard = () => {
  const { balancesQuery, isAuth, referralsQuery, rewardsConfigQuery, dailyRewardQuery, isInitialized } = useRewardsContext();
  const { nextAchievementText, isLoading: isBadgesLoading, badgesQuery } = useStreakBadges();
  const streakModal = useDisclosure();
  const isMobile = useIsMobile();

  const [ isError, setIsError ] = useState(false);

  useRedirectForInvalidAuthToken();

  useEffect(() => {
    if (!config.features.rewards.isEnabled || (isInitialized && !isAuth)) {
      window.location.assign('/');
    }
  }, [ isInitialized, isAuth ]);

  useEffect(() => {
    setIsError(balancesQuery.isError || referralsQuery.isError || rewardsConfigQuery.isError || dailyRewardQuery.isError);
  }, [ balancesQuery.isError, referralsQuery.isError, rewardsConfigQuery.isError, dailyRewardQuery.isError ]);

  if (!config.features.rewards.isEnabled) {
    return null;
  }

  return (
    <>
      <Flex gap={ 3 } justifyContent="space-between" mb={ 6 }>
        <PageTitle
          title="Dashboard"
          secondRow={ (
            <span>
              <Link external href={ `https://merits.blockscout.com/?tab=users&utm_source=${ config.chain.id }&utm_medium=text-banner` }>
                Explore the Merits Hub
              </Link>{ ' ' }
              to earn, spend, and learn more about the program.
            </span>
          ) }
          mb={ 0 }
        />
        { !isMobile && <AdBanner format="mobile" w="fit-content" flexShrink={ 0 } borderRadius="md" overflow="hidden"/> }
      </Flex>
      <Flex flexDirection="column" alignItems="flex-start" w="full" gap={ 6 }>
        { isError && <Alert status="error">Failed to load some data. Please try again later.</Alert> }
        <Flex gap={ 6 } flexDirection={{ base: 'column', md: 'row' }} w="full">
          <RewardsDashboardCard
            title="All Merits"
            description="Claim your daily Merits and any Merits received from referrals."
            contentDirection="column-reverse"
            cardValueStyle={{ minH: { base: '64px', md: '116px' } }}
            contentAfter={ <DailyRewardClaimButton/> }
            hint={ (
              <>
                Total number of Merits earned from all activities.{ ' ' }
                <Link external href="https://docs.blockscout.com/using-blockscout/merits">
                  More info on Merits
                </Link>
              </>
            ) }
          >
            <RewardsDashboardCardValue
              value={ balancesQuery.data?.total || 'N/A' }
              isLoading={ balancesQuery.isPending }
              withIcon
            />
          </RewardsDashboardCard>
          <RewardsDashboardCard
            title="Referrals"
            description="Total number of users who have joined the program using your code or referral link."
            contentDirection="column-reverse"
            cardValueStyle={{ minH: { base: '64px', md: '116px' } }}
          >
            <RewardsDashboardCardValue
              value={ referralsQuery.data?.referrals ?
                `${ referralsQuery.data?.referrals } user${ Number(referralsQuery.data?.referrals) === 1 ? '' : 's' }` :
                'N/A'
              }
              isLoading={ referralsQuery.isPending }
            />
          </RewardsDashboardCard>
          <RewardsDashboardCard
            title="Streak"
            description={
              `Current number of consecutive days you${ apos }ve claimed your daily Merits. The longer your streak, the more daily Merits you can earn.`
            }
            hint={ (
              <>
                See the{ ' ' }
                <Link external href="https://docs.blockscout.com/using-blockscout/merits/streak-rewards">docs</Link>{ ' ' }
                to learn how your streak number affects daily rewards
              </>
            ) }
            contentDirection="column-reverse"
            cardValueStyle={{ minH: { base: '64px', md: '116px' } }}
            contentAfter={ (
              <Button mt={ 3 } onClick={ streakModal.onOpen } loading={ isBadgesLoading }>
                Check achievements
              </Button>
            ) }
          >
            <RewardsDashboardCardValue
              value={ dailyRewardQuery.data?.streak ?
                `${ dailyRewardQuery.data?.streak } day${ Number(dailyRewardQuery.data?.streak) === 1 ? '' : 's' }` :
                'N/A' }
              isLoading={ dailyRewardQuery.isPending }
              bottomText={ nextAchievementText }
              isBottomTextLoading={ isBadgesLoading }
            />
          </RewardsDashboardCard>
        </Flex>
        <RoutedTabs
          w="full"
          tabs={ [
            {
              id: 'activity',
              title: 'Activity',
              component: <ActivityTab/>,
            },
            {
              id: 'referrals',
              title: 'Referrals',
              component: <ReferralsTab/>,
            },
            {
              id: 'resources',
              title: 'Resources',
              component: <ResourcesTab/>,
            },
          ] }
        />
      </Flex>
      { !isBadgesLoading && !dailyRewardQuery.isPending && (
        <RewardsStreakModal
          open={ streakModal.open }
          onOpenChange={ streakModal.onOpenChange }
          currentStreak={ Number(dailyRewardQuery.data?.streak || 0) }
          badges={ badgesQuery.data?.items }
        />
      ) }
    </>
  );
};

export default RewardsDashboard;
