import { Flex, Text, chakra } from '@chakra-ui/react';
import { useCallback, useMemo, useState } from 'react';

import { getFeaturePayload } from 'configs/app/features/types';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { useRewardsContext } from 'lib/contexts/rewards';
import dayjs from 'lib/date/dayjs';
import useIsMobile from 'lib/hooks/useIsMobile';
import { USER_ACTIVITY } from 'stubs/rewards';
import { Button } from 'toolkit/chakra/button';
import { Heading } from 'toolkit/chakra/heading';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Hint } from 'toolkit/components/Hint/Hint';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import { mdash } from 'toolkit/utils/htmlEntities';
import IconSvg from 'ui/shared/IconSvg';
import useProfileQuery from 'ui/snippets/auth/useProfileQuery';

import MeritsIcon from '../../MeritsIcon';
import RewardsActivityPassCard from '../RewardsActivityPassCard';
import RewardsInstancesModal from '../RewardsInstancesModal';
import RewardsTaskDetailsModal from '../RewardsTaskDetailsModal';

const feature = config.features.rewards;
const marketplaceFeature = getFeaturePayload(config.features.marketplace);

function getMaxAmount(rewards: Record<string, string> | undefined) {
  if (!rewards) {
    return 0;
  }

  const values = Object.values(rewards).map(Number);

  if (values.length === 0) {
    return 0;
  }

  return Math.max(...values);
}

export default function ActivityTab() {
  const { isAuth, rewardsConfigQuery } = useRewardsContext();
  const explorersModal = useDisclosure();
  const taskDetailsModal = useDisclosure();
  const isMobile = useIsMobile();
  const [ selectedTaskIndex, setSelectedTaskIndex ] = useState<number>(0);

  const profileQuery = useProfileQuery();
  const checkActivityPassQuery = useApiQuery('rewards:user_check_activity_pass', {
    queryOptions: {
      enabled: feature.isEnabled && isAuth && Boolean(profileQuery.data?.address_hash),
    },
    queryParams: {
      address: profileQuery.data?.address_hash ?? '',
    },
  });
  const activityQuery = useApiQuery('rewards:user_activity', {
    queryOptions: {
      enabled: isAuth && feature.isEnabled,
      placeholderData: USER_ACTIVITY,
    },
  });
  const instancesQuery = useApiQuery('rewards:instances', {
    queryOptions: { enabled: feature.isEnabled },
  });

  const period = useMemo(() => {
    if (!activityQuery.data) {
      return undefined;
    }

    const item = activityQuery.data.items[0];
    const startDate = dayjs(item.date).format('MMM D');
    const endDate = dayjs(item.end_date).format('MMM D, YYYY');

    return `${ startDate } - ${ endDate }`;
  }, [ activityQuery ]);

  const activities = useMemo(() => {
    const calcActivity = (type: string) => {
      const current = activityQuery.data?.items.find((item) => item.activity === type);
      const previous = activityQuery.data?.last_week.find((item) => item.activity === type);

      const currentAmount = Number(current?.amount || 0);
      const previousAmount = Number(previous?.amount || 0);
      const currentPercentile = (current?.percentile || 0) * 100;
      const previousPercentile = (previous?.percentile || 0) * 100;
      const amountDiff = Number((currentAmount - previousAmount).toFixed(2));
      const percentileDiff = Number((currentPercentile - previousPercentile).toFixed(2));

      return {
        amount: currentAmount,
        percentile: `${ currentPercentile }%`,
        amountDiff: `${ amountDiff >= 0 ? '+' : '' }${ amountDiff }`,
        percentileDiff: `${ percentileDiff >= 0 ? '+' : '' }${ percentileDiff }%`,
      };
    };

    return {
      transactions: calcActivity('sent_transactions'),
      contracts: calcActivity('verified_contracts'),
      usage: calcActivity('blockscout_usage'),
    };
  }, [ activityQuery.data ]);

  const tasks = useMemo(() => (
    [
      {
        title: 'Blockscout activity',
        description: (
          <>
            Use Blockscout tools like{ ' ' }
            <Link
              external={ !marketplaceFeature?.essentialDapps }
              href={ marketplaceFeature?.essentialDapps ?
                route({ pathname: '/apps' }) :
                'https://eth.blockscout.com/apps?utm_source=blockscout&utm_medium=transactions-task'
              }
            >
              Essential dapps
            </Link>, or{ ' ' }
            <Link href={ route({ pathname: '/verified-contracts' }) }>
              interact with smart contracts
            </Link>{ ' ' }
            to start earning Merits.
          </>
        ),
        percentile: activities.transactions?.percentile,
        percentileDiff: activities.transactions?.percentileDiff,
        amount: activities.transactions?.amount,
        amountDiff: activities.transactions?.amountDiff,
        maxAmount: getMaxAmount(rewardsConfigQuery.data?.rewards?.sent_transactions_activity_rewards),
      },
      {
        title: 'Contracts verification',
        description: (
          <>
            Log in and{ ' ' }
            <Link href={ route({ pathname: '/contract-verification' }) }>
              verify a smart contract
            </Link>{ ' ' }
            on the Blockscout explorer to earn Merits.
          </>
        ),
        percentile: activities.contracts?.percentile,
        percentileDiff: activities.contracts?.percentileDiff,
        amount: activities.contracts?.amount,
        amountDiff: activities.contracts?.amountDiff,
        maxAmount: getMaxAmount(rewardsConfigQuery.data?.rewards?.verified_contracts_activity_rewards),
      },
      {
        title: 'Blockscout usage',
        description: (
          <>
            Use Blockscout explorers in your daily routine { mdash } check transactions, explore addresses,
            or add tokens/networks to MetaMask via Blockscout.
          </>
        ),
        percentile: activities.usage?.percentile,
        percentileDiff: activities.usage?.percentileDiff,
        amount: activities.usage?.amount,
        amountDiff: activities.usage?.amountDiff,
        maxAmount: getMaxAmount(rewardsConfigQuery.data?.rewards?.blockscout_usage_activity_rewards),
      },
    ]
  ), [ rewardsConfigQuery, activities ]);

  const labels = {
    period: { text: `Period: ${ period }`, hint: 'Current Merits period. All metrics reset weekly' },
    performanceRank: { text: 'Performance rank', hint: 'Your rank within a task group compared to other users in the same period. Higher rank = more Merits.' },
    meritsEarned: { text: 'Merits earned', hint: 'Estimated Merits based on your current rank. Final amount may change' },
  };

  const labelComponents = Object.fromEntries(Object.entries(labels).map(([ key, value ], index) => [ key, (
    <Flex key={ index } flex={ 1 } alignItems="center" gap={ 1 } _first={{ minW: { base: 'auto', md: '200px' } }}>
      <Text
        textStyle={{ base: 'sm', md: 'xs' }}
        color={{ base: 'text.primary', md: 'text.secondary' }}
        fontWeight="500"
      >
        { value.text }
      </Text>
      <Hint label={ value.hint }/>
    </Flex>
  ) ]));

  const openTaskDetails = useCallback((index: number) => () => {
    setSelectedTaskIndex(index);
    taskDetailsModal.onOpen();
  }, [ taskDetailsModal ]);

  const isActivityDataLoading = activityQuery.isPlaceholderData || checkActivityPassQuery.isPending;

  if (checkActivityPassQuery.data && !checkActivityPassQuery.data.is_valid) {
    return <RewardsActivityPassCard/>;
  }

  return (
    <>
      <Flex
        p={{ base: 1.5, md: 2 }}
        border="1px solid"
        borderColor={{ _light: 'gray.200', _dark: 'whiteAlpha.200' }}
        borderRadius="lg"
        gap={{ base: 4, md: 10 }}
        flexDirection={{ base: 'column', md: 'row' }}
      >
        <Flex
          display={{ base: 'contents', md: 'flex' }}
          flexDirection="column"
          w="340px"
          p={ 3 }
          pr={ 0 }
        >
          <Flex flexDirection="column" p={{ base: 1.5, md: 0 }} pb={ 0 }>
            <Heading level="3" mb={ 2 }>Your activity</Heading>
            <Text textStyle="sm" mb={{ base: 2, md: 4 }}>
              Use Blockscout and related products daily to earn Merits. Check each task for details and how to get started.
            </Text>
            <Flex alignItems="center" gap={ 3 } mb={{ base: 0, md: 4 }}>
              <Button
                loadingSkeleton={ instancesQuery.isLoading }
                onClick={ explorersModal.onOpen }
              >
                Earn
              </Button>
              <Link
                external
                href="https://docs.blockscout.com/using-blockscout/merits/activity-pass"
                fontSize="md"
                fontWeight="500"
                textAlign="center"
              >
                Learn more
              </Link>
            </Flex>
          </Flex>
          <Flex
            flexDirection="column"
            gap={ 2.5 }
            mt="auto"
            order={{ base: 3, md: 'auto' }}
            px={{ base: 1.5, md: 0 }}
          >
            <IconSvg name="status/warning" boxSize={ 6 } color="icon.primary"/>
            <Text textStyle="sm">
              <chakra.span fontWeight="600">Your current Merit count is not final!</chakra.span><br/>
              Merits are calculated based on the activity of all users and may increase or decrease by the end of the weekly period.
            </Text>
          </Flex>
        </Flex>
        <Flex display={{ base: 'flex', md: 'none' }} justifyContent="space-between" px={ 3 }>
          <Flex alignItems="center" gap={ 1 }>
            <Text textStyle="sm" fontWeight="500">
              Period
            </Text>
            <Hint label={ labels.period.hint }/>
          </Flex>
          <Text textStyle="sm" fontWeight="500" color="text.secondary">
            { period }
          </Text>
        </Flex>
        <Flex
          display={{ base: 'contents', md: 'flex' }}
          flex={ 1 }
          flexDirection="column"
          gap={ 1 }
        >
          <Flex p={ 3 } gap={ 8 } display={{ base: 'none', md: 'flex' }}>
            { Object.values(labelComponents) }
          </Flex>
          <Flex flexDirection="column" gap={{ base: 1.5, md: 1 }}>
            { tasks.map((item, index) => (
              <Flex
                key={ index }
                flexDirection={{ base: 'column', md: 'row' }}
                px={ 3 }
                py={ 4 }
                gap={{ base: 6, md: 8 }}
                borderRadius={{ base: 'lg', md: '8px' }}
                backgroundColor={{ _light: 'gray.50', _dark: 'whiteAlpha.50' }}
              >
                <Flex
                  flex={ 1 }
                  flexDirection={{ base: 'row', md: 'column' }}
                  gap={ 2 }
                  alignItems={{ base: 'center', md: 'flex-start' }}
                  justifyContent={{ base: 'space-between', md: 'flex-start' }}
                  minW={{ base: 'auto', md: '200px' }}
                >
                  <Text textStyle="sm" fontWeight={{ base: '700', md: '500' }}>
                    { item.title }
                  </Text>
                  <Link
                    textStyle={{ base: 'sm', md: 'xs' }}
                    fontWeight={{ base: '400', md: '500' }}
                    onClick={ openTaskDetails(index) }
                  >
                    Task details
                  </Link>
                </Flex>
                <Flex display={{ base: 'flex', md: 'contents' }} gap={ 8 }>
                  <Flex flex={ 1 } flexDirection="column" gap={ 2 } alignItems="flex-start">
                    <Flex display={{ base: 'flex', md: 'none' }}>
                      { labelComponents.performanceRank }
                    </Flex>
                    <Skeleton loading={ isActivityDataLoading }>
                      <Heading level="3">
                        { item.percentile }
                      </Heading>
                    </Skeleton>
                    <Skeleton loading={ isActivityDataLoading }>
                      <Text textStyle={{ base: 'sm', md: 'xs' }} color="text.secondary" fontWeight="500">
                        { item.percentileDiff } vs { isMobile ? 'prev.' : 'previous' } week
                      </Text>
                    </Skeleton>
                  </Flex>
                  <Flex flex={ 1 } flexDirection="column" gap={ 2 } alignItems="flex-start">
                    <Flex display={{ base: 'flex', md: 'none' }}>
                      { labelComponents.meritsEarned }
                    </Flex>
                    <Skeleton
                      loading={ isActivityDataLoading }
                      display="flex"
                      alignItems="center"
                    >
                      <MeritsIcon boxSize={ 6 } mr={ 2 }/>
                      <Heading level="3" mr={{ base: 0, md: 2 }}>
                        { item.amount }
                      </Heading>
                      <Text textStyle="sm" color="gray.400" fontWeight="500" alignSelf="flex-end" display={{ base: 'none', md: 'inline' }}>
                        /{ item.maxAmount }
                      </Text>
                      <Heading level="3" display={{ base: 'inline', md: 'none' }} color="text.secondary">
                        /{ item.maxAmount }
                      </Heading>
                    </Skeleton>
                    <Skeleton loading={ isActivityDataLoading }>
                      <Text textStyle={{ base: 'sm', md: 'xs' }} color="text.secondary" fontWeight="500">
                        { item.amountDiff } vs { isMobile ? 'prev.' : 'previous' } week
                      </Text>
                    </Skeleton>
                  </Flex>
                </Flex>
              </Flex>
            )) }
          </Flex>
          <Flex
            p={{ base: 1.5, md: 3 }}
            order={{ base: 4, md: 'auto' }}
          >
            <Text textStyle="xs" color="text.secondary" fontWeight="500">
              Metrics are not updated in real time. Please allow up to one hour for your Performance Rank and earned Merits to reflect recent activity.
              If you experience any issues, feel free to reach out on{ ' ' }
              <Link external href="https://discord.gg/blockscout">
                Discord
              </Link>
            </Text>
          </Flex>
        </Flex>
      </Flex>
      <RewardsInstancesModal
        isOpen={ explorersModal.open }
        onClose={ explorersModal.onClose }
        items={ instancesQuery.data?.items }
      />
      <RewardsTaskDetailsModal
        isOpen={ taskDetailsModal.open }
        onClose={ taskDetailsModal.onClose }
        title={ tasks[selectedTaskIndex].title }
      >
        { tasks[selectedTaskIndex].description }
      </RewardsTaskDetailsModal>
    </>
  );
}
