import { Grid, chakra, GridItem } from '@chakra-ui/react';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';

interface ContainerProps {
  className?: string;
  animation?: string;
  children: React.ReactNode;
}

const Container = chakra(({ animation, children, className }: ContainerProps) => {
  return (
    <Grid
      w="100%"
      animation={ animation }
      rowGap={ 2 }
      columnGap={ 2 }
      gridTemplateColumns="86px auto"
      alignItems="start"
      paddingY={ 4 }
      borderColor="border.divider"
      borderTopWidth="1px"
      _last={{
        borderBottomWidth: '1px',
      }}
      className={ className }
      textStyle="sm"
    >
      { children }
    </Grid>
  );
});

interface LabelProps {
  className?: string;
  children: React.ReactNode;
  isLoading?: boolean;
}

const Label = chakra(({ children, className, isLoading }: LabelProps) => {
  return (
    <Skeleton
      className={ className }
      loading={ isLoading }
      fontWeight={ 500 }
      my="5px"
      justifySelf="start"
    >
      { children }
    </Skeleton>
  );
});

interface ValueProps {
  className?: string;
  children: React.ReactNode;
}

const Value = chakra(({ children, className }: ValueProps) => {
  return (
    <GridItem
      className={ className }
      py="5px"
      color="text.secondary"
      overflow="hidden"
    >
      { children }
    </GridItem>
  );
});

const ListItemMobileGrid = {
  Container,
  Label,
  Value,
};

export default ListItemMobileGrid;
