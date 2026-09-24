import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { RouteParams } from 'nextjs/routes';
import { route } from 'nextjs/routes';

import type { LinkProps } from 'toolkit/chakra/link';
import { Link } from 'toolkit/chakra/link';
import IconSvg from 'ui/shared/IconSvg';

interface Props extends LinkProps {
  query?: Record<string, string | Array<string> | undefined>;
  routeParams?: RouteParams;
  adaptive?: boolean;
}

const AdvancedFilterLink = ({ query, routeParams, adaptive = true, ...rest }: Props) => {
  return (
    <Link
      href={ route({ pathname: '/advanced-filter', query }, routeParams) }
      display="flex"
      alignItems="center"
      gap={ 1 }
      textStyle="sm"
      { ...rest }
    >
      <IconSvg name="advanced-filter" boxSize={ 5 }/>
      <chakra.span hideBelow={ adaptive ? 'lg' : undefined }>Advanced</chakra.span>
    </Link>
  );
};

export default React.memo(AdvancedFilterLink);
