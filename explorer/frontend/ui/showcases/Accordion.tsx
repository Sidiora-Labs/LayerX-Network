import React from 'react';

import { AccordionItemContent, AccordionItemTrigger, AccordionItem, AccordionRoot } from 'toolkit/chakra/accordion';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const items = [
  { value: 'first-item', title: 'First Item', text: 'Some value 1...' },
  { value: 'second-item', title: 'Second Item', text: 'Some value 2...' },
  { value: 'third-item', title: 'Third Item', text: 'Some value 3...' },
];

// https://eth-sepolia.k8s-dev.blockscout.com/address/0x00000000000000ADc04C56Bf30aC9d3c0aAF14dC?tab=read_write_contract
// https://base.blockscout.com/token/0x8f9C456C928a33a3859Fa283fb57B23c908fE843/instance/1924977?tab=metadata

const AccordionShowcase = () => {
  return (
    <Container value="accordion">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack>
          <Sample label="variant: outline">
            <AccordionRoot w="400px">
              { items.map((item, index) => (
                <AccordionItem key={ index } value={ item.value }>
                  <AccordionItemTrigger>{ item.title }</AccordionItemTrigger>
                  <AccordionItemContent>{ item.text }</AccordionItemContent>
                </AccordionItem>
              )) }
            </AccordionRoot>
          </Sample>
          <Sample label="variant: faq">
            <AccordionRoot w="400px" variant="faq">
              { items.map((item, index) => (
                <AccordionItem key={ index } value={ item.value }>
                  <AccordionItemTrigger variant="faq">{ item.title }</AccordionItemTrigger>
                  <AccordionItemContent>{ item.text }</AccordionItemContent>
                </AccordionItem>
              )) }
            </AccordionRoot>
          </Sample>
        </SamplesStack>

        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: md">
            <AccordionRoot w="400px">
              { items.map((item, index) => (
                <AccordionItem key={ index } value={ item.value }>
                  <AccordionItemTrigger>{ item.title }</AccordionItemTrigger>
                  <AccordionItemContent>{ item.text }</AccordionItemContent>
                </AccordionItem>
              )) }
            </AccordionRoot>
          </Sample>
          <Sample label="size: sm">
            <AccordionRoot w="400px" size="sm">
              { items.map((item, index) => (
                <AccordionItem key={ index } value={ item.value }>
                  <AccordionItemTrigger indicatorPlacement="start">{ item.title }</AccordionItemTrigger>
                  <AccordionItemContent>{ item.text }</AccordionItemContent>
                </AccordionItem>
              )) }
            </AccordionRoot>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(AccordionShowcase);
