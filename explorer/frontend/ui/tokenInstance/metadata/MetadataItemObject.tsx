import { Box } from '@chakra-ui/react';
import React from 'react';

import { AccordionItemContent, AccordionItemTrigger } from 'toolkit/chakra/accordion';

import MetadataAccordion from './MetadataAccordion';
import MetadataAccordionItem from './MetadataAccordionItem';
import MetadataAccordionItemTitle from './MetadataAccordionItemTitle';

interface Props {
  name: string;
  value: Record<string, unknown>;
  level: number;
}

const MetadataItemObject = ({ name, value, level }: Props) => {

  if (level >= 4) {
    return (
      <MetadataAccordionItem value={ name } level={ level } isFlat>
        <MetadataAccordionItemTitle name={ name }/>
        <Box whiteSpace="pre-wrap">{ JSON.stringify(value, undefined, 2) }</Box>
      </MetadataAccordionItem>
    );
  }

  return (
    <MetadataAccordionItem
      value={ name }
      flexDir={{ lg: 'column' }}
      alignItems="stretch"
      py={ 0 }
      isFlat
      level={ level }
    >
      <AccordionItemTrigger
        px={ 0 }
        py={ 2 }
        _hover={{ bgColor: 'inherit' }}
        fontSize="sm"
        textAlign="left"
        _expanded={{
          borderColor: 'border.divider',
          borderBottomWidth: '1px',
        }}
        indicatorPlacement="start"
      >
        <MetadataAccordionItemTitle name={ name }/>
      </AccordionItemTrigger>
      <AccordionItemContent p={ 0 }>
        <MetadataAccordion data={ value as Record<string, unknown> } level={ level + 1 }/>
      </AccordionItemContent>
    </MetadataAccordionItem>
  );
};

export default React.memo(MetadataItemObject);
