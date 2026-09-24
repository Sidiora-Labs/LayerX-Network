import {
  Text,
  HStack,
  Flex,
  Box,
} from '@chakra-ui/react';
import React from 'react';

import type { NavGroupItem } from 'types/client/navigation';

import IconSvg from 'ui/shared/IconSvg';

import LightningLabel from '../LightningLabel';
import NavLinkIcon from '../NavLinkIcon';
import useNavLinkStyleProps from '../useNavLinkStyleProps';
import { checkRouteHighlight } from '../utils';

type Props = {
  item: NavGroupItem;
  onClick: () => void;
  isExpanded?: boolean;
};

const NavLinkGroup = ({ item, onClick, isExpanded }: Props) => {
  const styleProps = useNavLinkStyleProps({ isActive: item.isActive, isExpanded });

  const isHighlighted = checkRouteHighlight(item.subItems);

  return (
    <Box as="li" listStyleType="none" w="100%" onClick={ onClick }>
      <Box
        { ...styleProps.itemProps }
        w="100%"
        px={ 2 }
        aria-label={ `${ item.text } link group` }
        color={ item.isActive ? 'link.navigation.fg.selected' : 'link.navigation.fg' }
        bgColor={ item.isActive ? 'link.navigation.bg.selected' : 'transparent' }
      >
        <Flex justifyContent="space-between" width="100%" alignItems="center" pr={ 1 }>
          <HStack gap={ 0 } overflow="hidden">
            <NavLinkIcon item={ item }/>
            <Text
              { ...styleProps.textProps }
              ml={ 3 }
            >
              { item.text }
            </Text>
            { isHighlighted && (<LightningLabel iconColor={ item.isActive ? 'link.navigation.bg.selected' : 'link.navigation.bg' }/>) }
          </HStack>
          <IconSvg name="arrows/east-mini" transform="rotate(180deg)" boxSize={ 6 }/>
        </Flex>
      </Box>
    </Box>
  );
};

export default NavLinkGroup;
