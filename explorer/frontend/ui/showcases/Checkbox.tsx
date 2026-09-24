import React from 'react';

import { Checkbox, CheckboxGroup } from 'toolkit/chakra/checkbox';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const CheckboxShowcase = () => {

  return (
    <Container value="checkbox">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack>
          <Sample label="variant: subtle">
            <Checkbox>Option 1</Checkbox>
            <Checkbox checked>Option 2</Checkbox>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: md">
            <Checkbox size="md">Option 1</Checkbox>
            <Checkbox size="md">Option 2</Checkbox>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Disabled</SectionHeader>
        <SamplesStack>
          <Sample label="disabled: true">
            <Checkbox disabled>Option 1</Checkbox>
            <Checkbox checked disabled>Option 2</Checkbox>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Read-only</SectionHeader>
        <SamplesStack>
          <Sample label="readOnly: true">
            <Checkbox readOnly>Option 1</Checkbox>
            <Checkbox checked readOnly>Option 2</Checkbox>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Orientation</SectionHeader>
        <SamplesStack>
          <Sample label="orientation: vertical">
            <CheckboxGroup orientation="vertical">
              <Checkbox value="1">Option 1</Checkbox>
              <Checkbox value="2">Option 2</Checkbox>
            </CheckboxGroup>
          </Sample>
          <Sample label="orientation: horizontal">
            <CheckboxGroup orientation="horizontal">
              <Checkbox value="1">Option 1</Checkbox>
              <Checkbox value="2">Option 2</Checkbox>
            </CheckboxGroup>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(CheckboxShowcase);
