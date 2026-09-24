import React from 'react';

import CopyToClipboard from 'ui/shared/CopyToClipboard';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const ClipboardShowcase = () => {

  return (
    <Container value="clipboard">
      <Section>
        <SectionHeader>Type</SectionHeader>
        <SamplesStack>
          <Sample label="type: text">
            <CopyToClipboard text="Hello, world!" type="text"/>
          </Sample>
          <Sample label="type: link">
            <CopyToClipboard text="Hello, world!" type="link"/>
          </Sample>
          <Sample label="type: share">
            <CopyToClipboard text="Hello, world!" type="share"/>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Loading</SectionHeader>
        <SamplesStack>
          <Sample label="loading: true">
            <CopyToClipboard text="Hello, world!" type="text" isLoading/>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(ClipboardShowcase);
