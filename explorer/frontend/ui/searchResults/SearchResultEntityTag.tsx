import React from 'react';

import type { AddressMetadataTagApi } from 'types/api/addressMetadata';

import highlightText from 'lib/highlightText';
import type { TagProps } from 'toolkit/chakra/tag';
import { Tag } from 'toolkit/chakra/tag';
import EntityTagIcon from 'ui/shared/EntityTags/EntityTagIcon';
import { getTagName } from 'ui/shared/EntityTags/utils';

interface Props extends TagProps {
  metadata: AddressMetadataTagApi;
  searchTerm: string;
  addressHash?: string;
}

const SearchResultEntityTag = ({ metadata, searchTerm, addressHash, ...rest }: Props) => {
  const name = getTagName(metadata, addressHash);

  return (
    <Tag
      { ...rest }
      startElement={ <EntityTagIcon data={ metadata } noColors/> }
    >
      <span dangerouslySetInnerHTML={{ __html: highlightText(name, searchTerm) }}/>
    </Tag>
  );
};

export default React.memo(SearchResultEntityTag);
