import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import { Badge } from 'toolkit/chakra/badge';
import { Heading } from 'toolkit/chakra/heading';
import { Hint } from 'toolkit/components/Hint/Hint';

type Props = {
  title: string;
  description: string | React.ReactNode;
  hint?: string | React.ReactNode;
  availableSoon?: boolean;
  blurFilter?: boolean;
  contentAfter?: React.ReactNode;
  contentDirection?: 'column' | 'column-reverse' | 'row';
  reverse?: boolean;
  children?: React.ReactNode;
  label?: string;
  isLoading?: boolean;
  cardValueStyle?: object;
};

const RewardsDashboardCard = ({
  title, description, availableSoon, contentAfter, cardValueStyle, hint,
  contentDirection = 'column', children, blurFilter, label, isLoading,
}: Props) => {
  return (
    <Flex
      as="section"
      flexDirection={{ base: contentDirection === 'row' ? 'column' : contentDirection, md: contentDirection }}
      justifyContent={ contentDirection === 'column-reverse' ? 'flex-end' : 'flex-start' }
      p={{ base: 1.5, md: 2 }}
      border="1px solid"
      borderColor={{ _light: 'gray.200', _dark: 'whiteAlpha.200' }}
      borderRadius="lg"
      gap={{ base: 4, md: contentDirection === 'row' ? 10 : 4 }}
      w={ contentDirection === 'row' ? 'full' : 'auto' }
      flex={ contentDirection !== 'row' ? 1 : '0 1 auto' }
    >
      <Flex
        flexDirection="column"
        gap={ 2 }
        px={{ base: 1.5, md: 3 }}
        pb={ contentDirection === 'column-reverse' ? { base: 1.5, md: 3 } : 0 }
        pt={ contentDirection === 'column-reverse' ? 0 : { base: 1.5, md: 3 } }
        w={{ base: 'full', md: contentDirection === 'row' ? '340px' : 'full' }}
        flex={ 1 }
      >
        { label && <Badge loading={ isLoading }>{ label }</Badge> }
        { title && (
          <Flex alignItems="center" gap={ 2 }>
            <Heading level="3">{ title }</Heading>
            { hint && <Hint label={ hint } tooltipProps={{ interactive: true }}/> }
            { availableSoon && <Badge colorPalette="blue">Available soon</Badge> }
          </Flex>
        ) }
        <Text as="div" fontSize="sm">
          { description }
        </Text>
        <Flex flexDirection="column" mt={ contentDirection === 'column-reverse' ? 'auto' : 0 }>
          { contentAfter }
        </Flex>
      </Flex>
      <Flex
        alignItems="center"
        justifyContent="space-around"
        borderRadius={{ base: 'lg', md: '8px' }}
        backgroundColor={{ _light: 'gray.50', _dark: 'whiteAlpha.50' }}
        minH={{ base: '104px', md: '128px' }}
        mt={ contentDirection === 'column' ? 'auto' : 0 }
        filter="auto"
        blur={ blurFilter ? '4px' : '0' }
        flex={ contentDirection === 'row' ? 1 : '0 1 auto' }
        { ...cardValueStyle }
      >
        { children }
      </Flex>
    </Flex>
  );
};

export default RewardsDashboardCard;
