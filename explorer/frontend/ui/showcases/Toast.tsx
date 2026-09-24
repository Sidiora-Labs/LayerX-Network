/* eslint-disable react/jsx-no-bind */
import React from 'react';

import { Button } from 'toolkit/chakra/button';
import { toaster } from 'toolkit/chakra/toaster';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';
import { TEXT } from './utils';

const ToastShowcase = () => {
  return (
    <Container value="toast">
      <Section>
        <SectionHeader>Type</SectionHeader>
        <SamplesStack>
          <Sample label="type: info">
            <Button onClick={ () => toaster.create({ title: 'Info', description: TEXT.slice(0, 100), type: 'info' }) }>
              Info
            </Button>
          </Sample>
          <Sample label="type: success">
            <Button onClick={ () => toaster.success({ title: 'Success', description: 'Toast content' }) }>
              Success
            </Button>
          </Sample>
          <Sample label="type: warning">
            <Button onClick={ () => toaster.create({ title: 'Warning', description: 'Toast content', type: 'warning' }) }>
              Warning
            </Button>
          </Sample>
          <Sample label="type: error">
            <Button onClick={ () => toaster.error({ title: 'Error', description: 'Toast content' }) }>
              Error
            </Button>
          </Sample>
          <Sample label="type: loading">
            <Button onClick={ () => toaster.loading({ title: 'Loading', description: 'Please wait for...' }) }>
              Loading
            </Button>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(ToastShowcase);
