import React from 'react';

import { ProgressCircleRoot, ProgressCircleRing, ProgressCircleValueText } from 'toolkit/chakra/progress-circle';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const ProgressCircleShowcase = () => {

  return (
    <Container value="progress-circle">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack >
          <Sample label="colorPalette: blue">
            <ProgressCircleRoot
              value={ 45 }
              colorPalette="blue"
            >
              <ProgressCircleRing/>
            </ProgressCircleRoot>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack >
          { ([ 'sm', 'md', 'lg' ] as const).map((size) => (
            <Sample key={ size } label={ `size: ${ size }` }>
              <ProgressCircleRoot
                value={ 45 }
                colorPalette="blue"
                size={ size }
              >
                <ProgressCircleRing/>
                { size === 'lg' && <ProgressCircleValueText/> }
              </ProgressCircleRoot>
            </Sample>
          )) }
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(ProgressCircleShowcase);
