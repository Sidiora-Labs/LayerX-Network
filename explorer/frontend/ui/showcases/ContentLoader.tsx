import React from 'react';

import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const ContentLoaderShowcase = () => {

  return (
    <Container value="content-loader">
      <Section>
        <SectionHeader>Variants</SectionHeader>
        <SamplesStack >
          <Sample label="default">
            <ContentLoader/>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(ContentLoaderShowcase);
