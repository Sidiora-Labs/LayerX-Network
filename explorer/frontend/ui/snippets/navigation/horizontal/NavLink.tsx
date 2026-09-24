import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { NavItem } from 'types/client/navigation';

import { route } from 'nextjs-routes';

import { isInternalItem } from 'lib/hooks/useNavItems';
import { Link } from 'toolkit/chakra/link';

import LightningLabel from '../LightningLabel';
import NavLinkIcon from '../NavLinkIcon';
import { checkRouteHighlight } from '../utils';

interface Props {
  className?: string;
  item: NavItem;
  noIcon?: boolean;
}

const NavLink = ({ className, item, noIcon }: Props) => {
  const isInternalLink = isInternalItem(item);

  const isActive = 'isActive' in item && item.isActive;

  const isHighlighted = checkRouteHighlight(item);

  return (
    <chakra.li
      listStyleType="none"
    >
      <Link
        className={ className }
        href={ isInternalLink ? route(item.nextRoute) : item.url }
        external={ !isInternalLink }
        display="flex"
        alignItems="center"
        variant="navigation"
        { ...(isActive ? { 'data-selected': true } : {}) }
        w="224px"
        px={ 2 }
        py="9px"
        textStyle="sm"
        fontWeight={ 500 }
        borderRadius="base"
      >
        { !noIcon && <NavLinkIcon item={ item } mr={ 3 }/> }
        <chakra.span>{ item.text }</chakra.span>
        { isHighlighted && (
          <LightningLabel
            iconColor={ isActive ? 'link.navigation.bg.selected' : 'link.navigation.bg.group' }
            position={{ lg: 'static' }}
            ml={{ lg: '2px' }}
            isCollapsed={ false }
          />
        ) }
      </Link>
    </chakra.li>
  );
};

export default React.memo(chakra(NavLink));
