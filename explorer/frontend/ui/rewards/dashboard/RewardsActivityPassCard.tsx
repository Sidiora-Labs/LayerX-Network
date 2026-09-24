import { Flex, Text } from '@chakra-ui/react';

import config from 'configs/app';
import { useRewardsContext } from 'lib/contexts/rewards';
import { useColorModeValue } from 'toolkit/chakra/color-mode';
import { Heading } from 'toolkit/chakra/heading';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';

export default function RewardsActivityPassCard() {
  const { rewardsConfigQuery } = useRewardsContext();
  const backgroundImage = useColorModeValue('/static/merits/cells.svg', '/static/merits/cells_dark.svg');

  const activityPassUrl = config.apis.rewards ?
    // eslint-disable-next-line max-len
    `${ config.apis.rewards.endpoint }/?tab=spend&id=${ rewardsConfigQuery.data?.rewards?.blockscout_activity_pass_id }&utm_source=blockscout&utm_medium=tasks` :
    undefined;

  return (
    <Flex
      p={{ base: 1.5, md: 2 }}
      border="1px solid"
      borderColor={{ _light: 'gray.200', _dark: 'whiteAlpha.200' }}
      borderRadius="lg"
      gap={{ base: 1, md: 10 }}
      flexDirection={{ base: 'column', md: 'row' }}
    >
      <Flex flex={ 1 } flexDirection="column" p={ 3 } gap={ 2 }>
        <Heading level="3">
          Activity pass
        </Heading>
        <Text textStyle="sm">
          Grab your{ ' ' }
          <Link external href={ activityPassUrl } loading={ rewardsConfigQuery.isLoading }>
            Activity pass
          </Link>{ ' ' }
          then engage with various Blockscout products and features to earn Merits every day!{ ' ' }
          <Link external href="https://docs.blockscout.com/using-blockscout/merits/activity-pass">
            Learn more
          </Link>
        </Text>
      </Flex>
      <Flex
        flex={{ base: 'none', md: 1 }}
        flexDirection={{ base: 'column', md: 'row' }}
        justifyContent="space-between"
        alignItems="center"
        h={{ base: '160px', md: '120px' }}
        pr={{ base: 0, md: 8 }}
        pl={{ base: 0, md: '86px' }}
        pt={{ base: 4, md: 0 }}
        pb={{ base: 3, md: 0 }}
        borderRadius="base"
        backgroundColor={{ _light: '#FFEFCE', _dark: '#E1910E' }}
        overflow="hidden"
        position="relative"
      >
        <Image
          src={ backgroundImage }
          alt="Background"
          width="268px"
          height="184px"
          position="absolute"
          top="-20px"
          left={{ base: 'calc(50% - 134px)', md: '-8px' }}
        />
        <Image
          src="/static/merits/activity_pass.svg"
          alt="Activity pass"
          width="79px"
          height="86px"
          zIndex={ 1 }
        />
        <Link
          external
          href={ activityPassUrl }
          variant="underlaid"
          fontWeight="500"
          backgroundColor={{ _light: '#FFD57C', _dark: '#FFBA0D' }}
          color="#2B1A3F"
          iconColor="rgba(43, 26, 63, 0.3)"
          _hover={{ color: 'hover' }}
          flexShrink={ 0 }
          zIndex={ 1 }
        >
          Grab Activity pass
        </Link>
      </Flex>
    </Flex>
  );
}
