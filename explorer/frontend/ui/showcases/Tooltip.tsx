import React from 'react';

import { Tooltip } from 'toolkit/chakra/tooltip';
import Utilization from 'ui/shared/Utilization/Utilization';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const TooltipShowcase = () => {
  return (
    <Container value="tooltip">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack>
          <Sample label="variant: regular">
            <Tooltip content="Tooltip content">
              <span>Default</span>
            </Tooltip>
            <Tooltip content="Tooltip content" interactive>
              <Utilization value={ 0.5 }/>
            </Tooltip>
          </Sample>
          <Sample label="variant: popover">
            <Tooltip content="Tooltip content" variant="popover">
              <span>Default</span>
            </Tooltip>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(TooltipShowcase);
