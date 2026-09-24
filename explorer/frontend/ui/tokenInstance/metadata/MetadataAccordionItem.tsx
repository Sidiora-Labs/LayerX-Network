import { chakra } from '@chakra-ui/react';
import React from 'react';

import { AccordionItem } from 'toolkit/chakra/accordion';

interface Props {
  children: React.ReactNode;
  level?: number;
  className?: string;
  isFlat?: boolean;
  value: string;
}

const MetadataAccordionItem = ({ children, className, level, isFlat, value }: Props) => {
  return (
    <AccordionItem
      value={ value }
      className={ className }
      display="flex"
      alignItems="flex-start"
      flexDir={{ base: 'column', lg: 'row' }}
      py={ 2 }
      pl={ isFlat ? 0 : 6 }
      columnGap={ 3 }
      borderColor="border.divider"
      wordBreak="break-all"
      rowGap={ 1 }
      _first={{
        borderTopWidth: level === 0 ? '1px' : '0px',
      }}
      _last={{
        borderBottomWidth: level === 0 ? '1px' : '0px',
      }}
    >
      { children }
    </AccordionItem>
  );
};

export default React.memo(chakra(MetadataAccordionItem));
