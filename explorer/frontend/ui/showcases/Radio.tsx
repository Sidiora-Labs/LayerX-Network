import React from 'react';

import { Radio, RadioGroup } from 'toolkit/chakra/radio';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const RadioShowcase = () => {

  return (
    <Container value="radio">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack>
          <Sample label="variant: subtle">
            <RadioGroup defaultValue="1">
              <Radio value="1">Option 1</Radio>
              <Radio value="2">Option 2</Radio>
            </RadioGroup>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: xs">
            <RadioGroup defaultValue="1" size="xs">
              <Radio value="1">Option 1</Radio>
              <Radio value="2">Option 2</Radio>
            </RadioGroup>
          </Sample>
          <Sample label="size: sm">
            <RadioGroup defaultValue="1" size="sm">
              <Radio value="1">Option 1</Radio>
              <Radio value="2">Option 2</Radio>
            </RadioGroup>
          </Sample>
          <Sample label="size: md">
            <RadioGroup defaultValue="1" size="md">
              <Radio value="1">Option 1</Radio>
              <Radio value="2">Option 2</Radio>
            </RadioGroup>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Orientation</SectionHeader>
        <SamplesStack>
          <Sample label="orientation: horizontal">
            <RadioGroup defaultValue="1" orientation="horizontal">
              <Radio value="1">Option 1</Radio>
              <Radio value="2">Option 2</Radio>
            </RadioGroup>
          </Sample>
          <Sample label="orientation: vertical">
            <RadioGroup defaultValue="1" orientation="vertical">
              <Radio value="1">Option 1</Radio>
              <Radio value="2">Option 2</Radio>
            </RadioGroup>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Disabled</SectionHeader>
        <SamplesStack>
          <Sample label="disabled: true">
            <RadioGroup defaultValue="1" disabled>
              <Radio value="1">Option 1</Radio>
              <Radio value="2">Option 2</Radio>
            </RadioGroup>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Read-only</SectionHeader>
        <SamplesStack>
          <Sample label="readOnly: true">
            <RadioGroup defaultValue="1" readOnly>
              <Radio value="1">Option 1</Radio>
              <Radio value="2">Option 2</Radio>
            </RadioGroup>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(RadioShowcase);
