import { Box, Flex, chakra } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import getPageType from 'lib/mixpanel/getPageType';
import * as mixpanel from 'lib/mixpanel/index';
import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import { PopoverBody, PopoverContent, PopoverRoot, PopoverTrigger } from 'toolkit/chakra/popover';
import { space } from 'toolkit/utils/htmlEntities';
import IconSvg from 'ui/shared/IconSvg';

import DeFiDropdownItem from './DeFiDropdownItem';

const feature = config.features.deFiDropdown;

const DeFiDropdown = () => {
  const router = useRouter();
  const source = getPageType(router.pathname);

  const handleClick = React.useCallback((content: string) => {
    mixpanel.logEvent(mixpanel.EventTypes.BUTTON_CLICK, { Content: `DeFi button: ${ content }`, Source: source });
  }, [ source ]);

  if (!feature.isEnabled) {
    return null;
  }

  const items = feature.items.map((item) => ({
    ...item,
    onClick: () => handleClick(item.text),
  }));

  return items.length > 1 ? (
    <PopoverRoot>
      <PopoverTrigger>
        <Button size="2xs" gap={ 0 }>
          <chakra.span display={{ base: 'none', lg: 'inline' }} whiteSpace="pre-wrap">
            Blockscout{ space }
          </chakra.span>
          DeFi
          <IconSvg name="arrows/east-mini" boxSize={ 4 } ml={ 1 } transform="rotate(-90deg)"/>
        </Button>
      </PopoverTrigger>
      <PopoverContent w="auto" minW={{ base: 'auto', lg: '132px' }}>
        <PopoverBody >
          <Flex flexDirection="column" gap={ 1 }>
            { items.map((item, index) => (
              <DeFiDropdownItem key={ index } item={ item }/>
            )) }
          </Flex>
        </PopoverBody>
      </PopoverContent>
    </PopoverRoot>
  ) : (
    <Link
      href={
        items[0].dappId ?
          route({
            pathname: items[0].isEssentialDapp ? '/essential-dapps/[id]' : '/apps/[id]',
            query: { id: items[0].dappId, action: 'connect' },
          }) :
          items[0].url
      }
      target={ items[0].dappId ? '_self' : '_blank' }
      asChild
    >
      <Button onClick={ items[0].onClick } size="2xs">
        { items[0].icon && <IconSvg name={ items[0].icon } boxSize={ 3 } mr={{ base: 0, sm: 1 }}/> }
        <Box display={{ base: 'none', sm: 'inline' }}>
          { items[0].text }
        </Box>
      </Button>
    </Link>
  );
};

export default chakra(React.memo(DeFiDropdown));
