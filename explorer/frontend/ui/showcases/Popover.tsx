import React from 'react';

import { Button } from 'toolkit/chakra/button';
import { PopoverRoot, PopoverTrigger, PopoverContent, PopoverBody } from 'toolkit/chakra/popover';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const PopoverShowcase = () => {
  return (
    <Container value="popover">
      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: sm">
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="dropdown">Trigger</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  Popover content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(PopoverShowcase);
