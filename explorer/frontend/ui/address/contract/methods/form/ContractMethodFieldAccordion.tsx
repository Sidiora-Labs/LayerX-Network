import { Box } from '@chakra-ui/react';
import React from 'react';

import { AccordionItem, AccordionItemContent, AccordionItemTrigger, AccordionRoot } from 'toolkit/chakra/accordion';
import ArrayButton from 'ui/shared/forms/ArrayButton';

export interface Props {
  label: string;
  level: number;
  children: React.ReactNode;
  onAddClick?: (event: React.MouseEvent<HTMLButtonElement>) => void;
  onRemoveClick?: (event: React.MouseEvent<HTMLButtonElement>) => void;
  index?: number;
  isInvalid?: boolean;
}

const ContractMethodFieldAccordion = ({ label, level, children, onAddClick, onRemoveClick, index, isInvalid }: Props) => {
  const bgColorLevel0 = { _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' };
  const bgColor = { _light: 'whiteAlpha.700', _dark: 'blackAlpha.700' };

  return (
    <AccordionRoot w="100%" bgColor={ level === 0 ? bgColorLevel0 : bgColor } borderRadius="base">
      <AccordionItem value="default" _first={{ borderTopWidth: 0 }} _last={{ borderBottomWidth: 0 }}>
        <AccordionItemTrigger
          indicatorPlacement="start"
          px="6px"
          py="6px"
          wordBreak="break-all"
          textAlign="left"
          _hover={{ bgColor: 'inherit' }}
        >
          <Box textStyle="sm" fontWeight={ 700 } mr="auto" color={ isInvalid ? 'text.error' : undefined }>
            { label }
          </Box>
          { onRemoveClick && index !== undefined && <ArrayButton index={ index } onClick={ onRemoveClick } type="remove"/> }
          { onAddClick && index !== undefined && <ArrayButton index={ index } onClick={ onAddClick } type="add" ml={ 1 }/> }
        </AccordionItemTrigger>
        <AccordionItemContent display="flex" flexDir="column" rowGap={ 1 } pl="18px" pr="6px">
          { children }
        </AccordionItemContent>
      </AccordionItem>
    </AccordionRoot>
  );
};

export default React.memo(ContractMethodFieldAccordion);
