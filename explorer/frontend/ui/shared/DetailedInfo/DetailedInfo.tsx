import type { GridItemProps, GridProps } from '@chakra-ui/react';
import { Flex, Grid, GridItem, Text } from '@chakra-ui/react';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { Hint } from 'toolkit/components/Hint/Hint';
import * as ContainerWithScrollY from 'ui/shared/ContainerWithScrollY';

export const ITEM_VALUE_LINE_HEIGHT = { base: '30px', lg: '32px' };

export const Container = (props: GridProps) => {
  return (
    <Grid
      columnGap={ 8 }
      rowGap={{ base: 0, lg: 3 }}
      templateColumns={{ base: 'minmax(0, 1fr)', lg: 'max-content minmax(728px, auto)' }}
      textStyle={{ base: 'sm', lg: 'md' }}
      { ...props }
    />
  );
};

interface ItemLabelProps extends GridItemProps {
  hint?: React.ReactNode;
  children: React.ReactNode;
  isLoading?: boolean;
  id?: string;
  hasScroll?: boolean;
}

const ItemLabelScrollText = () => (
  <Text fontWeight={ 500 } color="text.secondary" textStyle="xs" className="note" textAlign="right">
    Scroll to see more
  </Text>
);

export const ItemLabel = ({ hint, children, isLoading, id, hasScroll, ...rest }: ItemLabelProps) => {
  return (
    <GridItem
      id={ id }
      minH={ ITEM_VALUE_LINE_HEIGHT }
      _notFirst={{ mt: { base: 3, lg: 0 } }}
      { ...rest }
    >
      <Flex columnGap={{ base: 1, lg: 2 }} alignItems="flex-start" w="100%">
        { hint && <Hint label={ hint } isLoading={ isLoading } my={{ base: '5px', lg: '6px' }}/> }
        <Skeleton loading={ isLoading } fontWeight={{ base: 700, lg: 500 }} py={{ base: '5px', lg: '4px' }} flexGrow={ 1 }>
          { children }
          { hasScroll && <ItemLabelScrollText/> }
        </Skeleton>
      </Flex>
    </GridItem>
  );
};

interface ItemValueProps extends GridItemProps {
  children: React.ReactNode;
  multiRow?: boolean;
}

export const ItemValue = ({ children, multiRow = false, ...rest }: ItemValueProps) => {
  return (
    <GridItem
      display="flex"
      alignItems="center"
      pl={{ base: 6, lg: 0 }}
      minH={ ITEM_VALUE_LINE_HEIGHT }
      whiteSpace="nowrap"
      { ...(multiRow ? {
        flexWrap: 'wrap',
        lineHeight: ITEM_VALUE_LINE_HEIGHT,
      } : {}) }
      { ...rest }
    >
      { children }
    </GridItem>
  );
};

export const ItemValueWithScroll = ({ children, gradientHeight, onScrollVisibilityChange, ...rest }: ContainerWithScrollY.Props) => {
  return (
    <ItemValue position="relative" >
      <ContainerWithScrollY.default
        { ...rest }
        gradientHeight={ gradientHeight }
        onScrollVisibilityChange={ onScrollVisibilityChange }
      >
        { children }
      </ContainerWithScrollY.default>
    </ItemValue>
  );
};

export const ItemDivider = (props: GridItemProps) => {
  const { colSpan = { base: undefined, lg: 2 }, ...rest } = props;
  return (
    <GridItem
      colSpan={ colSpan }
      mt={{ base: 2, lg: 3 }}
      mb={{ base: 0, lg: 3 }}
      borderBottom="1px solid"
      borderColor="border.divider"
      { ...rest }
    />
  );
};
