import { Flex } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import React, { useCallback, useEffect, useMemo } from 'react';

import { useRewardsContext } from 'lib/contexts/rewards';
import { Button } from 'toolkit/chakra/button';
import { SECOND } from 'toolkit/utils/consts';
import splitSecondsInPeriods from 'ui/blockCountdown/splitSecondsInPeriods';

const DailyRewardClaimButton = () => {
  const queryClient = useQueryClient();
  const { balancesQuery, dailyRewardQuery, claim } = useRewardsContext();
  const [ isClaiming, setIsClaiming ] = React.useState(false);
  const [ timeLeft, setTimeLeft ] = React.useState<string>('');

  const dailyRewardValue = useMemo(() =>
    dailyRewardQuery.data ?
      Number(Number(dailyRewardQuery.data.total_reward).toFixed(2)) :
      0,
  [ dailyRewardQuery.data ]);

  const handleClaim = useCallback(async() => {
    setIsClaiming(true);
    try {
      await claim();
      await Promise.all([
        balancesQuery.refetch(),
        dailyRewardQuery.refetch(),
      ]);
      queryClient.invalidateQueries({ queryKey: [ 'rewards:user_badges' ] });
    } catch (error) {}
    setIsClaiming(false);
  }, [ claim, setIsClaiming, balancesQuery, dailyRewardQuery, queryClient ]);

  useEffect(() => {
    if (!dailyRewardQuery.data?.reset_at) {
      return;
    }

    // format the date to be compatible with the Date constructor
    const formattedDate = dailyRewardQuery.data.reset_at.replace(' ', 'T').replace(' UTC', 'Z');
    const target = new Date(formattedDate).getTime();

    let interval = 0;

    const updateCountdown = (target: number) => {
      const now = new Date().getTime();
      const difference = target - now;

      if (difference > 0) {
        const { hours, minutes, seconds } = splitSecondsInPeriods(Math.floor(difference / SECOND));
        setTimeLeft(`${ hours }:${ minutes }:${ seconds }`);
      } else {
        setTimeLeft('00:00:00');
        dailyRewardQuery.refetch();
        clearInterval(interval);
      }
    };

    updateCountdown(target);

    interval = window.setInterval(() => {
      updateCountdown(target);
    }, SECOND);

    return () => clearInterval(interval);
  }, [ dailyRewardQuery ]);

  const isLoading = isClaiming || dailyRewardQuery.isPending || dailyRewardQuery.isFetching;

  return !isLoading && !dailyRewardQuery.data?.available ? (
    <Flex
      h="40px"
      alignItems="center"
      justifyContent="center"
      borderRadius="base"
      color="gray.500"
      bgColor={{ _light: 'gray.200', _dark: 'gray.800' }}
      fontSize="md"
      fontWeight="600"
      cursor="default"
    >
      Next claim in { timeLeft || 'N/A' }
    </Flex>
  ) : (
    <Button onClick={ handleClaim } loading={ isLoading }>
      Claim { dailyRewardValue } Merits
    </Button>
  );
};

export default DailyRewardClaimButton;
