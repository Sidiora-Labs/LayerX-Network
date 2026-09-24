import { upperFirst } from 'es-toolkit';

import type { Metadata, MetadataAttributes } from 'types/client/token';

import dayjs from 'lib/date/dayjs';
import { SECOND } from 'toolkit/utils/consts';

function formatValue(value: string | number, display: string | undefined, trait: string | undefined): Pick<MetadataAttributes, 'value' | 'value_type'> {
  // https://docs.opensea.io/docs/metadata-standards#attributes
  switch (display) {
    case 'boost_number': {
      return {
        value: `+${ value } boost`,
      };
    }
    case 'boost_percentage': {
      return {
        value: `${ value }% boost`,
      };
    }
    case 'date': {
      return {
        value: dayjs(Number(value) * SECOND).format('YYYY-MM-DD'),
      };
    }
    default: {
      try {
        if (trait?.toLowerCase().includes('url') || value.toString().startsWith('http')) {
          const url = new URL(String(value));
          return {
            value: url.toString(),
            value_type: 'URL',
          };
        }
        throw new Error();
      } catch (error) {
        return {
          value: String(value),
        };
      }
    }
  }
}

export default function attributesParser(attributes: Array<unknown>): Metadata['attributes'] {
  return attributes
    .map((item) => {
      if (typeof item !== 'object' || !item) {
        return;
      }

      const value = (() => {
        if (!('value' in item)) {
          return;
        }
        switch (typeof item.value) {
          case 'string':
          case 'number':
            return item.value;
          case 'boolean':
            return String(item.value);
          case 'object':
            return JSON.stringify(item.value);
        }
      })();

      const trait = 'trait_type' in item && typeof item.trait_type === 'string' ? item.trait_type : undefined;
      const display = 'display_type' in item && typeof item.display_type === 'string' ? item.display_type : undefined;

      if (value === undefined) {
        return;
      }

      return {
        ...formatValue(value, display, trait),
        trait_type: upperFirst(trait || 'property'),
      };
    })
    .filter((item) => item?.value)
    .filter(Boolean);
}
