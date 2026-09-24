import type { BoxProps, StackProps, TabsContentProps } from '@chakra-ui/react';
import { Box, Code, Grid, HStack, VStack } from '@chakra-ui/react';
import React from 'react';

import { Heading } from 'toolkit/chakra/heading';
import { TabsContent } from 'toolkit/chakra/tabs';

export const Container = (props: TabsContentProps) => <TabsContent display="flex" flexDirection="column" gap={ 6 } w="100%" { ...props }/>;
export const Section = (props: BoxProps) => <Box { ...props } as="section"/>;
export const SectionHeader = ({ children }: { children: React.ReactNode }) => <Heading level="2" mb={ 4 }>{ children }</Heading>;
export const SectionSubHeader = ({ children }: { children: React.ReactNode }) => <Heading level="3" mb={ 3 } _notFirst={{ mt: 4 }}>{ children }</Heading>;
export const SamplesStack = ({ children }: { children: React.ReactNode }) => (
  <Grid
    rowGap={ 4 }
    columnGap={ 8 }
    gridTemplateColumns="fit-content(100%) 1fr"
    justifyItems="flex-start"
    alignItems="flex-start"
  >
    { children }
  </Grid>
);
export const Sample = ({ children, label, vertical, ...props }: { children: React.ReactNode; vertical?: boolean; label?: string } & StackProps) => {
  const Stack = vertical ? VStack : HStack;
  return (
    <>
      { label && <Code w="fit-content">{ label }</Code> }
      <Stack
        gap={ 3 }
        whiteSpace="pre-wrap"
        flexWrap="wrap"
        columnSpan={ label ? '1' : '2' }
        alignItems={ vertical ? 'flex-start' : 'center' }
        { ...props }
      >
        { children }
      </Stack>
    </>
  );
};
