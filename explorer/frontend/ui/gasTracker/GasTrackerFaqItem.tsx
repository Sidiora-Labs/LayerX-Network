import { Text } from '@chakra-ui/react';

import { AccordionItem, AccordionItemTrigger, AccordionItemContent } from 'toolkit/chakra/accordion';

interface Props {
  question: string;
  answer: string;
}

const GasTrackerFaqItem = ({ question, answer }: Props) => {
  return (
    <AccordionItem as="section" value={ question }>
      <AccordionItemTrigger variant="faq" >
        { question }
      </AccordionItemTrigger>
      <AccordionItemContent pb={ 4 } px={ 0 }>
        <Text color="text.secondary">{ answer }</Text>
      </AccordionItemContent>
    </AccordionItem>
  );
};

export default GasTrackerFaqItem;
