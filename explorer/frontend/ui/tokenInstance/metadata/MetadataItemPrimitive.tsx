import { Box } from '@chakra-ui/react';
import React from 'react';
import type { Primitive } from 'react-hook-form';

import urlParser from 'lib/token/metadata/urlParser';
import { Link } from 'toolkit/chakra/link';

import MetadataAccordionItem from './MetadataAccordionItem';
import MetadataAccordionItemTitle from './MetadataAccordionItemTitle';

interface PropsItem {
  itemValue: string;
  isItem: true;
  isFlat?: boolean;
}

interface PropsBox {}

type Props = {
  name?: string;
  value: Primitive;
  level: number;
} & (PropsItem | PropsBox);

const MetadataItemPrimitive = ({ name, value, level, ...rest }: Props) => {

  const content = (() => {
    switch (typeof value) {
      case 'string': {
        const url = urlParser(value);
        if (url) {
          return <Link external href={ url.toString() }>{ value }</Link>;
        }
        if (value === '') {
          return <div>&quot;&quot;</div>;
        }
      }
      // eslint-disable-next-line no-fallthrough
      default: {
        return <div>{ String(value) }</div>;
      }
    }
  })();

  if ('isItem' in rest) {
    return (
      <MetadataAccordionItem value={ rest.itemValue } level={ level } isFlat={ rest.isFlat }>
        { name && <MetadataAccordionItemTitle name={ name }/> }
        { content }
      </MetadataAccordionItem>
    );
  }

  return (
    <Box>
      { name && <MetadataAccordionItemTitle name={ name }/> }
      { content }
    </Box>
  );
};

export default React.memo(MetadataItemPrimitive);
