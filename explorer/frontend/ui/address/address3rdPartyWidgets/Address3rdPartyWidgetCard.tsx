import { Flex, Text, chakra, Separator } from '@chakra-ui/react';
import { useCallback } from 'react';

import type { Address3rdPartyWidget } from 'types/views/address';

import config from 'configs/app';
import * as mixpanel from 'lib/mixpanel/index';
import { Image } from 'toolkit/chakra/image';
import { LinkBox, LinkOverlay } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Hint } from 'toolkit/components/Hint/Hint';
import { ndash } from 'toolkit/utils/htmlEntities';
import IconSvg from 'ui/shared/IconSvg';

import useWidgetData from './useWidgetData';

type Props = {
  name: string;
  config: Address3rdPartyWidget | undefined;
  address: string;
  isLoading: boolean;
};

const chainId = config.chain.id || '';

function formatUrl(tpl: string, ctx: Record<string, string>) {
  return tpl.replace(/\{\s*(\w+)\s*\}/g, (_, key) => ctx[key] ?? '');
}

const Address3rdPartyWidgetCard = ({ name, config, address, isLoading }: Props) => {
  const { data, isLoading: isDataLoading } = useWidgetData(name, config?.valuePath, address, isLoading);

  const handleClick = useCallback(() => {
    mixpanel.logEvent(mixpanel.EventTypes.ADDRESS_WIDGET, { Name: name });
  }, [ name ]);

  if (!config) {
    return null;
  }

  const url = formatUrl(config.url, {
    address,
    addressLowercase: address.toLowerCase(),
    chainId: config.chainIds?.[chainId] ?? chainId,
  });

  const [ integer, decimal ] = data?.split('.') || [];

  const content = isLoading ? (
    <>
      <Skeleton loading w="88px" h="40px" mb={ 1 }/>
      <Skeleton loading w="178px" h="20px"/>
      <Separator mt={ 3 } mb={ 2 } borderColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.100' }}/>
      <Flex alignItems="center" gap={ 2 }>
        <Skeleton loading w="20px" h="20px"/>
        <Skeleton loading w="80px" h="20px"/>
      </Flex>
    </>
  ) : (
    <>
      <LinkOverlay href={ url } external onClick={ handleClick } noIcon/>
      <Skeleton loading={ isDataLoading } minW="88px" alignSelf="flex-start">
        { data ? (
          <Text
            textStyle="heading.xl"
            color={ integer === '0' && !decimal ? 'text.secondary' : 'text.primary' }
            textOverflow="ellipsis"
            whiteSpace="nowrap"
            overflow="hidden"
          >
            { integer }
            { decimal && (
              <>
                .
                <chakra.span color="text.secondary">
                  { decimal }
                </chakra.span>
              </>
            ) }
          </Text>
        ) : (
          <Text textStyle="heading.xl" color="text.secondary" opacity={ 0.2 }>{ ndash }</Text>
        ) }
      </Skeleton>
      <Flex alignItems="center" gap={ 1 } mt={ 1 }>
        <Text textStyle="sm">{ config.title }</Text>
        { config.hint && (
          <Hint
            label={ config.hint }
            tooltipProps={{ positioning: { placement: 'bottom' } }}
          />
        ) }
      </Flex>
      <Separator mt={ 3 } mb={ 2 } borderColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.100' }}/>
      <Flex alignItems="center" gap={ 2 }>
        <Image src={ config.icon } alt={ config.name } boxSize={ 5 }/>
        <Flex
          alignItems="center"
          justifyContent="space-between"
          flex={ 1 }
        >
          <Text
            textStyle="xs"
            color="text.secondary"
            _groupHover={{ color: 'hover' }}
          >
            { config.name }
          </Text>
          <IconSvg
            name="link_external"
            boxSize={ 3 }
            color="hover"
            display="none"
            _groupHover={{ display: 'block' }}
          />
        </Flex>
      </Flex>
    </>
  );

  return (
    <LinkBox
      as={ Flex }
      className="group"
      flexDirection="column"
      p={ 3 }
      cursor={ isLoading ? 'default' : 'pointer' }
      borderRadius="md"
      border="1px solid"
      borderColor={ isLoading ? { _light: 'blackAlpha.50', _dark: 'whiteAlpha.100' } : 'transparent' }
      bgColor={ isLoading ? 'transparent' : { _light: 'blackAlpha.50', _dark: 'whiteAlpha.100' } }
      transition="border-color 0.2s ease-in-out"
      _hover={ isLoading ? {} : {
        borderColor: { _light: 'blackAlpha.200', _dark: 'whiteAlpha.200' },
      } }
    >
      { content }
    </LinkBox>
  );
};

export default Address3rdPartyWidgetCard;
