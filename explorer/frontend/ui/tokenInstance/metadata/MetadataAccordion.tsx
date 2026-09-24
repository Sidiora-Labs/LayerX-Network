import React from 'react';

import { AccordionRoot } from 'toolkit/chakra/accordion';

import MetadataItemArray from './MetadataItemArray';
import MetadataItemObject from './MetadataItemObject';
import MetadataItemPrimitive from './MetadataItemPrimitive';
import { sortFields } from './utils';

interface Props {
  data: Record<string, unknown>;
  level?: number;
}

const MetadataAccordion = ({ data, level = 0 }: Props) => {
  const ml = (() => {
    if (level === 0) {
      return 0;
    }

    if (level === 1) {
      return 126;
    }

    return 24;
  })();

  const isFlat = Object.entries(data).every(([ , value ]) => typeof value !== 'object');

  const renderItem = React.useCallback((name: string, value: unknown) => {
    switch (typeof value) {
      case 'string':
      case 'number':
      case 'boolean': {
        return (
          <MetadataItemPrimitive
            key={ name }
            name={ name }
            value={ value }
            isItem
            isFlat={ isFlat }
            itemValue={ String(value) }
            level={ level }
          />
        );
      }

      case 'object': {
        if (value === null) {
          return (
            <MetadataItemPrimitive
              key={ name }
              name={ name }
              value={ value }
              isItem
              itemValue={ String(value) }
              isFlat={ isFlat }
              level={ level }
            />
          );
        }
        if (Array.isArray(value) && value.length > 0) {
          return <MetadataItemArray key={ name } name={ name } value={ value } level={ level }/>;
        }

        if (Object.keys(value).length > 0) {
          return <MetadataItemObject key={ name } name={ name } value={ value as Record<string, unknown> } level={ level }/>;
        }
      }
      // eslint-disable-next-line no-fallthrough
      default: {
        return (
          <MetadataItemPrimitive
            key={ name }
            name={ name }
            value={ String(value) }
            isItem
            itemValue={ String(value) }
            isFlat={ isFlat }
            level={ level }
          />
        );
      }
    }
  }, [ level, isFlat ]);

  const entries = Object.entries(data).sort(sortFields);

  return (
    <AccordionRoot
      multiple
      textStyle="sm"
      ml={{ base: level === 0 ? 0 : 6, lg: `${ ml }px` }}
      defaultValue={ level === 0 ? entries.map(([ key ]) => key) : undefined }
    >
      { entries.map(([ key, value ]) => renderItem(key, value)) }
    </AccordionRoot>
  );
};

export default React.memo(MetadataAccordion);
