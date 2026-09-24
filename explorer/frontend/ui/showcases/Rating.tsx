import React from 'react';

import { Rating } from 'toolkit/chakra/rating';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const RatingShowcase = () => {
  return (
    <Container value="rating">
      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: md">
            <Rating defaultValue={ 3 } label={ [ 'Very bad', 'Bad', 'Average', 'Good', 'Excellent' ] }/>
          </Sample>
        </SamplesStack>
      </Section>
      <Section>
        <SectionHeader>Read-only</SectionHeader>
        <SamplesStack>
          <Sample label="readOnly: true">
            <Rating defaultValue={ 3 } label={ [ 'Very bad', 'Bad', 'Average', 'Good', 'Excellent' ] } readOnly/>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(RatingShowcase);
