import React from 'react';

import type { LinkProps } from 'toolkit/chakra/link';
import { Link } from 'toolkit/chakra/link';

interface Props extends LinkProps {};

const SearchBarSuggestItemLink = React.forwardRef<HTMLAnchorElement, Props>(({ children, ...rest }, ref) => {
  return (
    <Link
      ref={ ref }
      py={ 3 }
      px={{ base: 0, lg: 1 }}
      display="flex"
      flexDir="column"
      alignItems="stretch"
      variant="plain"
      rowGap={ 2 }
      borderColor="border.divider"
      borderBottomWidth="1px"
      _last={{
        borderBottomWidth: '0',
      }}
      _hover={{
        bgColor: { _light: 'blue.50', _dark: 'gray.800' },
      }}
      textStyle="sm"
      _first={{
        mt: 2,
      }}
      { ...rest }
    >
      { children }
    </Link>
  );
});

export default SearchBarSuggestItemLink;
