import React from 'react';

import type { LinkProps } from 'toolkit/chakra/link';
import { Link } from 'toolkit/chakra/link';

interface Props extends LinkProps {
  children: React.ReactNode;
}

const SearchResultListItem = ({ children, ...rest }: Props) => {
  return (
    <Link
      className="group"
      variant="plain"
      display="flex"
      alignItems={{ base: 'flex-start', lg: 'center' }}
      flexDir={{ base: 'column', lg: 'row' }}
      rowGap={{ base: 3, lg: 0 }}
      columnGap={ 3 }
      pl={ 1 }
      pr={ 4 }
      py={ 3 }
      _hover={{
        color: 'hover',
      }}
      borderBottomWidth="1px"
      borderBottomStyle="solid"
      borderBottomColor="border.divider"
      _last={{
        borderBottomWidth: '0',
      }}
      { ...rest }
    >
      { children }
    </Link>
  );
};

export default React.memo(SearchResultListItem);
