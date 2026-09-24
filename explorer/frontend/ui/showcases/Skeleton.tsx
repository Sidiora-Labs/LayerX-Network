import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const SkeletonShowcase = () => {

  return (
    <Container value="skeleton">
      <Section>
        <SectionHeader>Variants</SectionHeader>
        <SamplesStack >
          <Sample label="default">
            <Skeleton loading>
              <span>Skeleton</span>
            </Skeleton>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(SkeletonShowcase);
