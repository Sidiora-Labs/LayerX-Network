import { chakra } from '@chakra-ui/react';
import React from 'react';

import useIsInitialLoading from 'lib/hooks/useIsInitialLoading';
import useIsMobile from 'lib/hooks/useIsMobile';
import { IconButton } from 'toolkit/chakra/icon-button';
import type { SelectRootProps } from 'toolkit/chakra/select';
import { SelectContent, SelectItem, SelectRoot, SelectControl, SelectValueText } from 'toolkit/chakra/select';
import IconSvg from 'ui/shared/IconSvg';

export interface Props extends SelectRootProps {
  isLoading?: boolean;
}

const Sort = (props: Props) => {
  const { collection, isLoading, ...rest } = props;
  const isMobile = useIsMobile();
  const isInitialLoading = useIsInitialLoading(isLoading);

  const trigger = (() => {
    if (isMobile) {
      return (
        <SelectControl triggerProps={{ asChild: true }} noIndicator>
          <IconButton
            loadingSkeleton={ isInitialLoading }
            aria-label="sort"
            size="md"
            variant="dropdown"
            disabled={ isLoading }
          >
            <IconSvg name="arrows/up-down"/>
          </IconButton>
        </SelectControl>
      );
    }

    return (
      <SelectControl
        loading={ isInitialLoading }
        _hover={{ color: 'hover' }}
        _open={{ color: 'hover' }}
      >
        <chakra.span
          flexShrink={ 0 }
          fontWeight="normal"
          color={{ _light: 'blackAlpha.600', _dark: 'whiteAlpha.600' }}
          _groupHover={{ color: 'inherit' }}
          _groupExpanded={{ color: 'inherit' }}
        >
          Sort by
        </chakra.span>
        <SelectValueText
          color={{ _light: 'blackAlpha.800', _dark: 'whiteAlpha.800' }}
          _groupHover={{ color: 'inherit' }}
          _groupExpanded={{ color: 'inherit' }}
        />
      </SelectControl>
    );
  })();

  return (
    <SelectRoot collection={ collection } w="fit-content" variant="plain" disabled={ isLoading } { ...rest }>
      { trigger }
      <SelectContent>
        { collection.items.map((item) => (
          <SelectItem item={ item } key={ item.value }>
            { item.label }
          </SelectItem>
        )) }
      </SelectContent>
    </SelectRoot>
  );
};

export default React.memo(chakra(Sort)) as typeof Sort;
