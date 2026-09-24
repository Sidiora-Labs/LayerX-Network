import { HStack, chakra, Separator } from '@chakra-ui/react';
import React from 'react';

import type { NavGroupItem } from 'types/client/navigation';

import { Link } from 'toolkit/chakra/link';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import IconSvg from 'ui/shared/IconSvg';

import LightningLabel from '../LightningLabel';
import { checkRouteHighlight } from '../utils';
import NavLink from './NavLink';
interface Props {
  item: NavGroupItem;
}

const NavLinkGroup = ({ item }: Props) => {
  const { open, onOpenChange } = useDisclosure();

  const isHighlighted = checkRouteHighlight(item.subItems);
  const hasGroups = item.subItems.some((subItem) => Array.isArray(subItem));

  const content = hasGroups ? (
    <HStack separator={ <Separator/> } alignItems="stretch">
      { item.subItems.map((subItem, index) => {
        if (!Array.isArray(subItem)) {
          return <NavLink key={ subItem.text } item={ subItem }/>;
        }

        return (
          <chakra.ul key={ index } display="flex" flexDir="column" rowGap={ 1 }>
            { subItem.map((navItem) => <NavLink key={ navItem.text } item={ navItem }/>) }
          </chakra.ul>
        );
      }) }
    </HStack>
  ) : (
    <chakra.ul display="flex" flexDir="column" rowGap={ 1 }>
      { item.subItems.map((subItem) => {
        if (Array.isArray(subItem)) {
          return null;
        }
        return <NavLink key={ subItem.text } item={ subItem }/>;
      }) }
    </chakra.ul>
  );

  return (
    <Tooltip
      variant="popover"
      content={ content }
      onOpenChange={ onOpenChange }
      lazyMount={ false }
      positioning={{
        placement: 'bottom',
        offset: { mainAxis: 8 },
      }}
      interactive
    >
      <Link
        as="li"
        listStyleType="none"
        display="flex"
        alignItems="center"
        px={ 2 }
        py={ 1.5 }
        textStyle="sm"
        fontWeight={ 500 }
        variant="navigation"
        { ...(item.isActive ? { 'data-selected': true } : {}) }
        { ...(open ? { 'data-active': true } : {}) }
        borderRadius="base"
      >
        { item.text }
        { isHighlighted && (
          <LightningLabel
            iconColor={ item.isActive ? 'link.navigation.bg.selected' : 'link.navigation.bg.group' }
            position={{ lg: 'static' }}
            ml={{ lg: '2px' }}
          />
        ) }
        <IconSvg name="arrows/east-mini" boxSize={ 5 } transform="rotate(-90deg)" ml={ 1 }/>
      </Link>
    </Tooltip>
  );
};

export default React.memo(NavLinkGroup);
