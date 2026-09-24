import { Flex, Text } from '@chakra-ui/react';

import { Heading } from 'toolkit/chakra/heading';
import IconSvg from 'ui/shared/IconSvg';
import type { IconName } from 'ui/shared/IconSvg';

const STEPS = [
  {
    text: 'Click Connect Wallet on the top right or enter an address in the search bar.',
    icon: 'wallet' as IconName,
  },
  {
    text: 'Inspect your approvals by using the network selection, sorting and filtering options.',
    icon: 'search' as IconName,
  },
  {
    text: 'Revoke the approvals that you no longer use to prevent unwanted access to your funds.',
    icon: 'return' as IconName,
  },
];

export default function StartScreen() {
  return (
    <Flex flexDir="column" w="full" gap={{ base: 3, md: 6 }}>
      <Heading level="3">
        How to revoke your approvals
      </Heading>
      <Flex flexDir={{ base: 'column', md: 'row' }} gap={{ base: 2, md: 6 }}>
        { STEPS.map((step, index) => (
          <Flex
            key={ index }
            flexDir={{ base: 'column', md: 'row' }}
            alignItems={{ base: 'flex-start', md: 'center' }}
            p={ 6 }
            borderRadius="md"
            bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
            flex={ 1 }
            gap={ 6 }
          >
            <IconSvg name={ step.icon } boxSize={ 6 }/>
            <Text textStyle="sm">
              { step.text }
            </Text>
          </Flex>
        )) }
      </Flex>
    </Flex>
  );
}
