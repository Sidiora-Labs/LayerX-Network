import { Box, Center, Flex, Grid } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import { route } from 'nextjs/routes';

import useApiQuery from 'lib/api/useApiQuery';
import { useMultichainContext } from 'lib/contexts/multichain';
import dayjs from 'lib/date/dayjs';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import getQueryParamString from 'lib/router/getQueryParamString';
import { Button } from 'toolkit/chakra/button';
import { Heading } from 'toolkit/chakra/heading';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import { downloadBlob } from 'toolkit/utils/file';
import BlockCountdownTimer from 'ui/blockCountdown/BlockCountdownTimer';
import createGoogleCalendarLink from 'ui/blockCountdown/createGoogleCalendarLink';
import createIcsFileBlob from 'ui/blockCountdown/createIcsFileBlob';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import IconSvg from 'ui/shared/IconSvg';
import StatsWidget from 'ui/shared/stats/StatsWidget';
import Time from 'ui/shared/time/Time';

import CapybaraRunner from '../games/CapybaraRunner';

type Props = {
  hideCapybaraRunner?: boolean;
};

const BlockCountdown = ({ hideCapybaraRunner }: Props) => {
  const multichainContext = useMultichainContext();
  const router = useRouter();
  const height = getQueryParamString(router.query.height);

  const { data, isPending, isError, error } = useApiQuery('general:block_countdown', {
    queryParams: {
      module: 'block',
      action: 'getblockcountdown',
      blockno: height,
    },
  });

  const handleAddToAppleCalClick = React.useCallback(() => {
    if (!data?.result?.EstimateTimeInSec) {
      return;
    }
    const fileBlob = createIcsFileBlob({ blockHeight: height, date: dayjs().add(Number(data.result.EstimateTimeInSec), 's'), multichainContext });
    downloadBlob(fileBlob, `Block #${ height } creation event.ics`);
  }, [ data?.result?.EstimateTimeInSec, height, multichainContext ]);

  const handleTimerFinish = React.useCallback(() => {
    window.location.assign(route({ pathname: '/block/[height_or_hash]', query: { height_or_hash: height } }, multichainContext));
  }, [ height, multichainContext ]);

  React.useEffect(() => {
    if (!isError && !isPending && !data.result) {
      handleTimerFinish();
    }
  }, [ data?.result, handleTimerFinish, isError, isPending ]);

  if (isError) {
    throwOnResourceLoadError({ isError, error, resource: 'general:block_countdown' });
  }

  if (isPending || !data?.result) {
    return <Center h="100%"><ContentLoader/></Center>;
  }

  return (
    <Center h="100%" alignItems={{ base: 'flex-start', lg: 'center' }}>
      <Flex flexDir="column" w="fit-content" maxW={{ base: '100%', lg: '700px', xl: '1000px' }}>
        <Flex columnGap={ 8 } alignItems="flex-start" justifyContent={{ base: 'space-between', lg: undefined }} w="100%">
          <Box maxW={{ base: 'calc(100% - 65px - 32px)', lg: 'calc(100% - 125px - 32px)' }}>
            <Heading
              level="1"
            >
              <TruncatedText text={ `Block #${ height }` } w="100%"/>
            </Heading>
            <Box mt={ 2 } color="text.secondary">
              <Box fontWeight={ 600 }>Estimated target date</Box>
              <Time timestamp={ dayjs().add(Number(data.result.EstimateTimeInSec), 's').valueOf() }/>
            </Box>
            <Flex columnGap={ 2 } mt={ 3 }>
              <Link
                external
                variant="underlaid"
                textStyle="sm"
                px={ 2 }
                display="inline-flex"
                href={ createGoogleCalendarLink({ blockHeight: height, timeFromNow: Number(data.result.EstimateTimeInSec), multichainContext }) }
              >
                <Image src="/static/google_calendar.svg" alt="Google calendar logo" boxSize={ 5 } mr={ 2 }/>
                <span>Google</span>
              </Link>
              <Button
                variant="plain"
                px={ 2 }
                size="sm"
                fontWeight="normal"
                color="link.primary"
                _hover={{ color: 'link.primary.hover' }}
                bgColor="link.underlaid.bg"
                display="inline-flex"
                onClick={ handleAddToAppleCalClick }
              >
                <Image src="/static/apple_calendar.svg" alt="Apple calendar logo" boxSize={ 5 }/>
                <span>Apple</span>
              </Button>
            </Flex>
          </Box>
          <Box position="relative">
            <IconSvg
              name="block"
              w={{ base: '65px', lg: '125px' }}
              h={{ base: '75px', lg: '140px' }}
              color={{ _light: 'gray.300', _dark: 'gray.600' }}
              flexShrink={ 0 }
            />
            { multichainContext?.chain && (
              <ChainIcon
                data={ multichainContext.chain }
                position="absolute"
                bottom={{ base: '5px', lg: '6px' }}
                right={{ base: '45px', lg: '86px' }}
                boxSize={{ lg: '60px' }}
                bgColor="bg.primary"
                borderRadius="full"
              />
            ) }
          </Box>
        </Flex>
        { data.result.EstimateTimeInSec && (
          <BlockCountdownTimer
            value={ Math.ceil(Number(data.result.EstimateTimeInSec)) }
            onFinish={ handleTimerFinish }
          />
        ) }
        <Grid gridTemplateColumns="repeat(2, calc(50% - 4px))" columnGap={ 2 } mt={ 2 }>
          <StatsWidget label="Remaining blocks" value={ data.result.RemainingBlock } icon="apps"/>
          <StatsWidget label="Current block" value={ data.result.CurrentBlock } icon="block"/>
        </Grid>
        { !hideCapybaraRunner && <CapybaraRunner/> }
      </Flex>
    </Center>
  );
};

export default React.memo(BlockCountdown);
