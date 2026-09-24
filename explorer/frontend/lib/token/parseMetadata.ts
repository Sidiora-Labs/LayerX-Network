import type { TokenInstance } from 'types/api/token';
import type { Metadata } from 'types/client/token';

import attributesParser from './metadata/attributesParser';

export default function parseMetadata(raw: TokenInstance['metadata'] | undefined): Metadata | undefined {
  if (!raw) {
    return;
  }

  const parsed: Metadata = {};

  if ('name' in raw && typeof raw.name === 'string') {
    parsed.name = raw.name;
  }

  if ('description' in raw && typeof raw.description === 'string') {
    parsed.description = raw.description;
  }

  if ('attributes' in raw && Array.isArray(raw.attributes)) {
    parsed.attributes = attributesParser(raw.attributes);
  }

  if (Object.keys(parsed).length === 0) {
    return;
  }

  return parsed;
}
