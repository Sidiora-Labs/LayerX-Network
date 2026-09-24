import { Flex } from '@chakra-ui/react';

import { useRewardsContext } from 'lib/contexts/rewards';
import { Skeleton } from 'toolkit/chakra/skeleton';

import RewardsReadOnlyInputWithCopy from '../../RewardsReadOnlyInputWithCopy';
import RewardsDashboardCard from '../RewardsDashboardCard';

export default function ReferralsTab() {
  const { rewardsConfigQuery, referralsQuery } = useRewardsContext();

  return (
    <RewardsDashboardCard
      title="Referral program"
      description={ (
        <>
          Refer friends and boost your Merits! You receive a{ ' ' }
          <Skeleton as="span" loading={ rewardsConfigQuery.isPending }>
            { rewardsConfigQuery.data?.rewards?.referral_share ?
              `${ Number(rewardsConfigQuery.data.rewards.referral_share) * 100 }%` :
              'N/A'
            }
          </Skeleton>
          { ' ' }bonus on all Merits earned by your referrals.
        </>
      ) }
      contentDirection="row"
    >
      <Flex
        flex={ 1 }
        gap={{ base: 2, lg: 6 }}
        px={{ base: 4, lg: 6 }}
        py={{ base: 4, lg: 0 }}
        flexDirection={{ base: 'column', lg: 'row' }}
      >
        <RewardsReadOnlyInputWithCopy
          label="Referral link"
          value={ referralsQuery.data?.link || 'N/A' }
          isLoading={ referralsQuery.isPending }
          flex={ 2 }
        />
        <RewardsReadOnlyInputWithCopy
          label="Referral code"
          value={ referralsQuery.data?.code || 'N/A' }
          isLoading={ referralsQuery.isPending }
          flex={ 1 }
        />
      </Flex>
    </RewardsDashboardCard>
  );
}
