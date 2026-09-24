import { Text } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import type { ExternalSearchItem as ExternalSearchItemType } from 'lib/search/externalSearch';
import { Link } from 'toolkit/chakra/link';

const zetaChainFeature = config.features.zetachain;

function getIndefiniteArticle(phrase: string): 'a' | 'an' {
  const trimmed = (phrase || '').trim().toLowerCase();
  if (!trimmed) return 'a';

  return 'aeiou'.includes(trimmed[0]) ? 'an' : 'a';
}

interface Props {
  item: ExternalSearchItemType;
}

const ExternalSearchItem = ({ item }: Props) => {
  if (!zetaChainFeature.isEnabled || !item) {
    return null;
  }

  const url = item.url;

  return (
    <>
      <Text color="text.secondary">
        It looks like you are searching for { getIndefiniteArticle(item.name) } { item.name }. This information is best served by the external explorer.
      </Text>
      <Link href={ url } external mt={ 4 }>
        Click here to be redirected
      </Link>
    </>
  );
};

export default React.memo(ExternalSearchItem);
