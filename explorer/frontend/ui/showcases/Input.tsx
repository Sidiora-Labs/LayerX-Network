import React from 'react';

import { Field } from 'toolkit/chakra/field';
import { Input } from 'toolkit/chakra/input';
import { InputGroup } from 'toolkit/chakra/input-group';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import IconSvg from 'ui/shared/IconSvg';

import { Section, Container, SectionHeader, SamplesStack, Sample, SectionSubHeader } from './parts';

const InputShowcase = () => {
  return (
    <Container value="input">
      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: sm">
            <Input type="text" placeholder="Name" size="sm"/>
          </Sample>
          <Sample label="size: md">
            <Input type="text" placeholder="Name" size="md"/>
          </Sample>
          <Sample label="size: lg">
            <Input type="text" placeholder="Name" size="lg"/>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack>
          <Sample label="variant: outline" maxW="300px">
            <Input type="text" placeholder="Name"/>
            <Input type="text" placeholder="Name (disabled)" disabled/>
            <Input type="text" placeholder="Name (readOnly)" readOnly/>
            <Input type="text" placeholder="Name (invalid)" data-invalid/>
            <Input type="text" placeholder="Name (invalid)" data-invalid value="duck"/>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Input group</SectionHeader>
        <SamplesStack>
          <Sample label="with end element">
            <Field label="Referral code" required floating size="lg" w="300px" flexShrink={ 0 }>
              <InputGroup endElement={ <IconSvg name="copy" boxSize={ 5 }/> } endElementProps={{ px: 3 }}>
                <Input/>
              </InputGroup>
            </Field>
          </Sample>
          <Sample label="with start element">
            <InputGroup startElement={ <IconSvg name="collection" boxSize={ 5 }/> } startElementProps={{ px: 2 }}>
              <Input placeholder="Type in something"/>
            </InputGroup>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Examples</SectionHeader>
        <SectionSubHeader>Filter input</SectionSubHeader>
        <SamplesStack>
          <Sample>
            <FilterInput placeholder="Search by method name"/>
            <FilterInput placeholder="Search by method name" loading/>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(InputShowcase);
