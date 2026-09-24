import { GridItem } from '@chakra-ui/react';
import React from 'react';

import { Heading } from 'toolkit/chakra/heading';

interface Props {
  children: React.ReactNode;
}

const TokenInfoFormSectionHeader = ({ children }: Props) => {
  return (
    <GridItem colSpan={{ base: 1, lg: 2 }} mt={ 3 }>
      <Heading level="2">
        { children }
      </Heading>
    </GridItem>
  );
};

export default TokenInfoFormSectionHeader;
