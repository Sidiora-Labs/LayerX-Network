import React from 'react';

import { Progress } from 'toolkit/chakra/progress';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const ProgressShowcase = () => {

  return (
    <Container value="progress">
      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack >
          { ([ 'sm', 'md', 'lg', 'xl' ] as const).map((size) => (
            <Sample key={ size } label={ `size: ${ size }` }>
              <Progress
                w="200px"
                min={ 0 }
                max={ 100 }
                value={ 45 }
                size={ size }
              />
            </Sample>
          )) }
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(ProgressShowcase);
