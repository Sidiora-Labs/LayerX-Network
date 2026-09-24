import { Spinner } from '@chakra-ui/react';
import React from 'react';

import { Section, Container, SectionHeader, SamplesStack, Sample, SectionSubHeader } from './parts';

const SpinnerShowcase = () => {

  return (
    <Container value="spinner">
      <Section>
        <SectionHeader>Spinner</SectionHeader>
        <SectionSubHeader>Sizes</SectionSubHeader>
        <SamplesStack>
          { ([ 'xs', 'sm', 'md', 'lg', 'xl' ] as const).map((size) => (
            <Sample key={ size } label={ `size: ${ size }` }>
              <Spinner size={ size }/>
            </Sample>
          )) }
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(SpinnerShowcase);
