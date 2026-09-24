import { Text, Flex, HStack, VStack, Separator, Box, chakra } from '@chakra-ui/react';
import { BigNumber } from 'bignumber.js';
import React from 'react';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import * as mixpanel from 'lib/mixpanel';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressMultichainButton from 'ui/address/details/AddressMultichainButton';
import AdBanner from 'ui/shared/ad/AdBanner';
import IconSvg from 'ui/shared/IconSvg';
import SimpleValue from 'ui/shared/value/SimpleValue';
import { DEFAULT_ACCURACY_USD } from 'ui/shared/value/utils';

import { formatPercentage } from './utils';

const multichainBalanceFeature = config.features.multichainButton;

const TOP_TOKENS_COLORS = [
  [ 'purple.300', 'pink.300', 'blackAlpha.300' ],
  [ 'purple.500', 'pink.600', 'whiteAlpha.300' ],
];

const getBgColor = (index: number) => {
  return {
    _light: TOP_TOKENS_COLORS[0][index],
    _dark: TOP_TOKENS_COLORS[1][index],
  };
};

interface Props {
  addressHash: string;
  netWorth?: string;
  isLoading: boolean;
  topTokens?: Array<{ symbol: string; share: number }>;
}

const MultichainAddressPortfolioNetWorth = ({ addressHash, netWorth, isLoading, topTokens }: Props) => {
  const isMobile = useIsMobile();

  const handleMultichainClick = React.useCallback(() => {
    mixpanel.logEvent(mixpanel.EventTypes.BUTTON_CLICK, { Content: 'Multichain', Source: 'address' });
  }, []);

  const topTokensContent = (() => {
    if (!topTokens) {
      return (
        <chakra.span color="text.secondary">There are no tokens at this address</chakra.span>
      );
    }

    return (
      <>
        <Skeleton loading={ isLoading } w={{ base: '100%', lg: '225px' }} h={ 3 } display="flex" alignItems="center" borderRadius="full" overflow="hidden">
          { topTokens.map((token, index) => (
            <Box
              key={ token.symbol }
              h="100%"
              w={ `${ token.share * 100 }%` }
              bgColor={ getBgColor(token.symbol === 'Others' ? 2 : index) }
              minW="1px"
            />
          )) }
        </Skeleton>
        <HStack flexWrap="wrap">
          { topTokens.map((token, index) => (
            <HStack key={ token.symbol }>
              <Skeleton
                boxSize={ 4 }
                borderRadius="full"
                loading={ isLoading }
                bgColor={ !isLoading ? getBgColor(token.symbol === 'Others' ? 2 : index) : undefined }
              />
              <Skeleton loading={ isLoading } fontWeight={ 600 } whiteSpace="pre">
                <span>{ token.symbol }</span>
                <chakra.span color="text.secondary"> { formatPercentage(token.share) }</chakra.span>
              </Skeleton>
            </HStack>
          )) }
        </HStack>
      </>
    );

  })();

  return (
    <HStack alignItems="center" w="full" h={{ base: 'auto', lg: '100px' }}>
      <VStack
        flexGrow={ 1 }
        borderRadius="base"
        overflow="hidden"
        h="100%"
        rowGap="1px"
      >
        <Flex
          alignItems={{ base: 'flex-start', lg: 'center' }}
          bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.100' }}
          flexBasis="50%"
          w="full"
          p={ 3 }
          gap={ 3 }
          whiteSpace="pre"
          flexDirection={{ base: 'column', lg: 'row' }}
          textStyle="sm"
        >
          <Flex alignItems="center">
            <IconSvg name="wallet" boxSize={ 5 } flexShrink={ 0 } color="icon.primary"/>
            <Text ml={ 2 } fontWeight={ 500 }>Total net worth</Text>
            <Text color="text.secondary"> (without NFT)</Text>
          </Flex>
          <Flex >
            <SimpleValue
              value={ BigNumber(netWorth ?? 0) }
              prefix="$"
              loading={ isLoading }
              fontWeight={ 600 }
              accuracy={ DEFAULT_ACCURACY_USD }
              color={ !netWorth ? 'text.secondary' : undefined }
            />
            { multichainBalanceFeature.isEnabled && (
              <>
                <Separator mx={ 3 } height="16px" orientation="vertical"/>
                <HStack gap={ 3 }>
                  { multichainBalanceFeature.providers.map((item) => (
                    <AddressMultichainButton
                      key={ item.name }
                      item={ item }
                      addressHash={ addressHash }
                      onClick={ handleMultichainClick }
                    />
                  )) }
                </HStack>
              </>
            ) }
          </Flex>
        </Flex>
        <Flex
          alignItems={{ base: 'flex-start', lg: 'center' }}
          bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.100' }}
          flexBasis="50%"
          w="full"
          p={ 3 }
          gap={ 3 }
          flexDirection={{ base: 'column', lg: 'row' }}
          textStyle="xs"
        >
          { topTokensContent }
        </Flex>
      </VStack>
      { !isMobile && <AdBanner format="mobile" w="fit-content"/> }
    </HStack>
  );
};

export default React.memo(MultichainAddressPortfolioNetWorth);
