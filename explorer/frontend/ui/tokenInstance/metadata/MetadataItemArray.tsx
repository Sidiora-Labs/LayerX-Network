import { Flex } from '@chakra-ui/react';
import React from 'react';

import { AccordionItemContent, AccordionItemTrigger } from 'toolkit/chakra/accordion';

import MetadataAccordionItem from './MetadataAccordionItem';
import MetadataAccordionItemTitle from './MetadataAccordionItemTitle';
import MetadataItemPrimitive from './MetadataItemPrimitive';

interface Props {
  name: string;
  value: Array<unknown>;
  level: number;
}

const MetadataItemArray = ({ name, value, level }: Props) => {

  return (
    <MetadataAccordionItem
      value={ name }
      flexDir={{ lg: 'column' }}
      alignItems="stretch"
      pl={{ base: 0, lg: 0 }}
      py={ 0 }
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
      <AccordionItemContent p={ 0 } ml={{ base: 6, lg: level === 0 ? '126px' : 6 }}>
        { value.map((item, index) => {
          const content = (() => {
            switch (typeof item) {
              case 'string':
              case 'number':
              case 'boolean': {
                return <MetadataItemPrimitive name={ name } value={ item } level={ level }/>;
              }
              case 'object': {
                if (item) {
                  if (Array.isArray(item)) {
                    return <span>{ JSON.stringify(item, undefined, 2) }</span>;
                  } else {
                    return Object.entries(item).map(([ name, value ], index) => {
                      return (
                        <Flex key={ index } columnGap={ 3 }>
                          <MetadataAccordionItemTitle name={ name } fontWeight={ 400 } w={{ base: '90px' }}/>
                          <MetadataItemPrimitive
                            value={ typeof value === 'object' ? JSON.stringify(value, undefined, 2) : value }
                            level={ level }
                          />
                        </Flex>
                      );
                    });
                  }
                } else {
                  return <span>{ String(item) }</span>;
                }
              }
              default: {
                return <span>{ String(item) }</span>;
              }
            }
          })();

          return (
            <Flex
              key={ index }
              py={ 2 }
              _notFirst={{ borderColor: 'border.divider', borderTopWidth: '1px' }}
              flexDir="column"
              rowGap={ 2 }
            >
              { content }
            </Flex>
          );
        }) }
      </AccordionItemContent>
    </MetadataAccordionItem>
  );
};

export default React.memo(MetadataItemArray);
