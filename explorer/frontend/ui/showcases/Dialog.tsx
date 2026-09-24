import { noop } from 'es-toolkit';
import React from 'react';

import { Button } from 'toolkit/chakra/button';
import { DialogActionTrigger, DialogBody, DialogContent, DialogFooter, DialogHeader, DialogRoot, DialogTitle, DialogTrigger } from 'toolkit/chakra/dialog';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';
import { TEXT } from './utils';

const DialogShowcase = () => {

  return (
    <Container value="dialog">
      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: sm">
            <DialogRoot size="sm">
              <DialogTrigger asChild>
                <Button size="sm">
                  Open Dialog
                </Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <DialogTitle>Dialog Title</DialogTitle>
                </DialogHeader>
                <DialogBody>
                  <p>{ TEXT }</p>
                </DialogBody>
                <DialogFooter>
                  <Button>Save</Button>
                  <DialogActionTrigger asChild>
                    <Button variant="link">I'll do it later</Button>
                  </DialogActionTrigger>
                </DialogFooter>
              </DialogContent>
            </DialogRoot>
          </Sample>

          <Sample label="size: md">
            <DialogRoot size="md">
              <DialogTrigger asChild>
                <Button size="sm">
                  Open Dialog
                </Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader onBackToClick={ noop }>
                  <DialogTitle>Dialog Title</DialogTitle>
                </DialogHeader>
                <DialogBody>
                  <p>{ TEXT }</p>
                </DialogBody>
                <DialogFooter>
                  <DialogActionTrigger asChild>
                    <Button variant="outline">Cancel</Button>
                  </DialogActionTrigger>
                  <Button>Save</Button>
                </DialogFooter>
              </DialogContent>
            </DialogRoot>
          </Sample>

          <Sample label="size: full">
            <DialogRoot size="full">
              <DialogTrigger asChild>
                <Button size="sm">
                  Open Dialog
                </Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <DialogTitle>Dialog Title</DialogTitle>
                </DialogHeader>
                <DialogBody>
                  <p>{ TEXT }</p>
                </DialogBody>
                <DialogFooter>
                  <DialogActionTrigger asChild>
                    <Button variant="outline">Cancel</Button>
                  </DialogActionTrigger>
                  <Button>Save</Button>
                </DialogFooter>
              </DialogContent>
            </DialogRoot>
          </Sample>
        </SamplesStack>
      </Section>

    </Container>
  );
};

export default React.memo(DialogShowcase);
