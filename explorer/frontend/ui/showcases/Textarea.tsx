import { Box } from '@chakra-ui/react';
import React from 'react';

import { Field } from 'toolkit/chakra/field';
import { Textarea } from 'toolkit/chakra/textarea';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';
import { TEXT } from './utils';

const TextareaShowcase = () => {

  return (
    <Container value="textarea">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack >
          <Sample label="variant: outline" w="360px">
            <Field label="Description" required floating size="2xl">
              <Textarea/>
            </Field>
            <Field label="Description" required floating size="2xl">
              <Textarea value={ TEXT }/>
            </Field>
            <Field label="Description (invalid)" required floating invalid size="2xl">
              <Textarea value={ TEXT }/>
            </Field>
            <Field label="Description (readOnly)" required floating readOnly size="2xl">
              <Textarea value={ TEXT }/>
            </Field>
            <Field label="Description (disabled)" required floating disabled size="2xl">
              <Textarea value={ TEXT }/>
            </Field>
            <Box bgColor={{ _light: 'blackAlpha.200', _dark: 'whiteAlpha.200' }} p={ 4 } borderRadius="base" w="100%">
              <Field label="Description" required floating size="2xl">
                <Textarea value={ TEXT }/>
              </Field>
            </Box>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(TextareaShowcase);
