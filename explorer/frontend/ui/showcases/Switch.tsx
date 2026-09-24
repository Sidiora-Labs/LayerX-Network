import React from 'react';

import { Switch } from 'toolkit/chakra/switch';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const SwitchShowcase = () => {

  return (
    <Container value="switch">
      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: sm">
            <Switch size="sm">
              Show duck
            </Switch>
          </Sample>
          <Sample label="size: md">
            <Switch size="md">
              Show duck
            </Switch>
          </Sample>
          <Sample label="size: lg">
            <Switch size="lg" defaultChecked>
              Show duck
            </Switch>
          </Sample>
        </SamplesStack>
      </Section>
      <Section>
        <SectionHeader>Disabled</SectionHeader>
        <SamplesStack>
          <Sample label="disabled: true">
            <Switch disabled>
              Show duck
            </Switch>
          </Sample>
        </SamplesStack>
      </Section>
      <Section>
        <SectionHeader>Direction</SectionHeader>
        <SamplesStack>
          <Sample label="direction: rtl">
            <Switch size="sm" direction="rtl">
              Show duck
            </Switch>
          </Sample>
          <Sample label="direction: ltr">
            <Switch size="md" direction="ltr">
              Show duck
            </Switch>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(SwitchShowcase);
