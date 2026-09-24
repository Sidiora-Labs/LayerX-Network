import { Box } from '@chakra-ui/react';
import React from 'react';

import type { NavLink } from './types';

import { Link } from 'toolkit/chakra/link';
import IconSvg from 'ui/shared/IconSvg';

const UserProfileContentNavLink = ({ href, icon, text, onClick }: NavLink) => {
  return (
    <Link
      href={ href }
      display="flex"
      alignItems="center"
      columnGap={ 3 }
      py="14px"
      onClick={ onClick }
      variant="menu"
    >
      <IconSvg name={ icon } boxSize={ 5 } flexShrink={ 0 }/>
      <Box textStyle="sm" fontWeight="500">{ text }</Box>
    </Link>
  );
};

export default React.memo(UserProfileContentNavLink);
