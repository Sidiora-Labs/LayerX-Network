import React from 'react';

import { CloseButton } from 'toolkit/chakra/close-button';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const CloseButtonShowcase = () => {

  return (
    <Container value="close-button">
      <Section>
        <SectionHeader>Variants</SectionHeader>
        <SamplesStack>
          <Sample label="variant: default">
            <CloseButton/>
            <CloseButton data-hover/>
            <CloseButton disabled/>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: md">
            <CloseButton size="md" outline="1px dashed lightpink"/>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(CloseButtonShowcase);
